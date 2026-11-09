//! The browser store's backend: one IndexedDB database per game, named by
//! the store name, holding one object store, `files`, whose keys are the
//! file names and whose values are the bytes as `Uint8Array`s.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use js_sys::{Array, Promise, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
  DomException, IdbDatabase, IdbObjectStore, IdbRequest, IdbTransaction, IdbTransactionMode,
  IdbVersionChangeEvent,
};

use super::SaveError;
use super::browser::{BrowserStorage, IdbBackend, IdbFailure, IdbOp, open_then_read};

/// The object store the files are kept in
const FILES: &str = "files";
/// The database's version: 1 made the object store
const VERSION: u32 = 1;
/// How long the start-up waits for the database to open before the game
/// starts with saves unavailable
pub(crate) const OPEN_TIMEOUT: Duration = Duration::from_secs(2);

thread_local! {
  /// The open databases, by name: JavaScript objects, which stay on the
  /// page's one thread while the store is shared as a `Storage`
  static DATABASES: RefCell<HashMap<String, IdbDatabase>> = RefCell::new(HashMap::new());
}

/// One game's IndexedDB database
pub(crate) struct IdbStore {
  name: String,
}

impl IdbBackend for IdbStore {
  fn commit(&self, ops: Vec<IdbOp>, done: Box<dyn FnOnce(Result<(), IdbFailure>) + Send>) {
    let transaction = DATABASES.with_borrow(|databases| {
      databases.get(&self.name).map(|database| {
        database.transaction_with_str_and_mode(FILES, IdbTransactionMode::Readwrite)
      })
    });
    let transaction = match transaction {
      Some(Ok(transaction)) => transaction,
      Some(Err(error)) => return done(Err(failure(&error))),
      None => return done(Err(IdbFailure::Other("the database is not open".into()))),
    };
    if let Err(error) = queue(&transaction, ops) {
      // The requests already made are dropped with the transaction
      let _ = transaction.abort();
      return done(Err(failure(&error)));
    }
    let finished = Promise::new(&mut |resolve, reject| {
      transaction.set_oncomplete(Some(&resolve));
      transaction.set_onabort(Some(&reject));
    });
    wasm_bindgen_futures::spawn_local(async move {
      let result = match JsFuture::from(finished).await {
        Ok(_) => Ok(()),
        Err(_) => Err(transaction.error().map_or_else(
          || IdbFailure::Other("the browser aborted the commit".into()),
          |exception| dom_failure(&exception),
        )),
      };
      done(result);
    });
  }
}

/// Makes the transaction's requests, in order
fn queue(transaction: &IdbTransaction, ops: Vec<IdbOp>) -> Result<(), JsValue> {
  let files = transaction.object_store(FILES)?;
  for op in ops {
    match op {
      IdbOp::Put(name, bytes) => {
        files.put_with_key(
          &Uint8Array::from(bytes.as_slice()),
          &JsValue::from_str(&name),
        )?;
      }
      IdbOp::Delete(name) => {
        files.delete(&JsValue::from_str(&name))?;
      }
    }
  }
  Ok(())
}

/// The game's store: its database opened within `limit`, else `Unavailable`
/// (storage blocked, or the browser slow to answer), then every file in it
/// read, however long that takes
pub(crate) async fn open_in_time(
  name: &str,
  limit: Duration,
) -> Result<BrowserStorage<IdbStore>, SaveError> {
  open_then_read(open_database(name), sleep(limit), |database| {
    read_store(name, database)
  })
  .await
}

/// Opens the game's database, making it on the first run
async fn open_database(name: &str) -> Result<IdbDatabase, SaveError> {
  let factory = web_sys::window()
    .and_then(|window| window.indexed_db().ok().flatten())
    .ok_or(SaveError::Unavailable)?;
  let request = factory
    .open_with_u32(name, VERSION)
    .map_err(|error| open_error(name, &error))?;
  // Freed once called (an existing database never calls it, and keeps it for
  // the page's life); it stays with the request if the open is abandoned at
  // the time-out, so a late first open still makes the object store
  let upgrade = request.clone();
  let on_upgrade = Closure::once_into_js(move |_: IdbVersionChangeEvent| {
    let made = upgrade
      .result()
      .and_then(JsCast::dyn_into::<IdbDatabase>)
      .and_then(|database| database.create_object_store(FILES));
    if let Err(error) = made {
      crate::engine_warn!(
        module: "Saves",
        "could not make the browser's save store: {:?}",
        failure(&error)
      );
    }
  });
  request.set_onupgradeneeded(Some(on_upgrade.unchecked_ref()));
  settle(&request)
    .await
    .and_then(JsCast::dyn_into)
    .map_err(|error| open_error(name, &error))
}

/// Every file in the open database, as the store's mirror; the database is
/// kept for the store's commits
async fn read_store(
  name: &str,
  database: IdbDatabase,
) -> Result<BrowserStorage<IdbStore>, SaveError> {
  let files = read_all(&database)
    .await
    .map_err(|error| open_error(name, &error))?;
  DATABASES.with_borrow_mut(|databases| databases.insert(name.to_string(), database));
  Ok(BrowserStorage::new(
    IdbStore {
      name: name.to_string(),
    },
    files,
  ))
}

/// Every file in the database: its names and bytes, read in one transaction
async fn read_all(database: &IdbDatabase) -> Result<Vec<(String, Vec<u8>)>, JsValue> {
  let transaction = database.transaction_with_str(FILES)?;
  let files: IdbObjectStore = transaction.object_store(FILES)?;
  // Both requests are made before either is awaited, so no event is missed
  let names = settle(&files.get_all_keys()?);
  let bytes = settle(&files.get_all()?);
  let names: Array = names.await?.dyn_into()?;
  let bytes: Array = bytes.await?.dyn_into()?;
  // Both come in key order
  Ok(
    names
      .iter()
      .zip(bytes.iter())
      .filter_map(|(name, bytes)| {
        let bytes = bytes.dyn_into::<Uint8Array>().ok()?;
        Some((name.as_string()?, bytes.to_vec()))
      })
      .collect(),
  )
}

/// The request's result once it succeeds, else its error. Its handlers are
/// set before this returns
fn settle(request: &IdbRequest) -> impl Future<Output = Result<JsValue, JsValue>> + use<> {
  let finished = Promise::new(&mut |resolve, reject| {
    request.set_onsuccess(Some(&resolve));
    request.set_onerror(Some(&reject));
  });
  let request = request.clone();
  async move {
    match JsFuture::from(finished).await {
      Ok(_) => request.result(),
      Err(_) => Err(
        request
          .error()
          .ok()
          .flatten()
          .map_or_else(|| JsValue::from_str("the request failed"), JsValue::from),
      ),
    }
  }
}

/// Resolves after `duration`, through the window's `setTimeout`; at once
/// where there is no window
async fn sleep(duration: Duration) {
  let millis = i32::try_from(duration.as_millis()).unwrap_or(i32::MAX);
  let elapsed = Promise::new(&mut |resolve, _| {
    let set = web_sys::window()
      .map(|window| window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, millis));
    if !matches!(set, Some(Ok(_))) {
      let _ = resolve.call0(&JsValue::NULL);
    }
  });
  let _ = JsFuture::from(elapsed).await;
}

fn open_error(name: &str, error: &JsValue) -> SaveError {
  let reason = match failure(error) {
    IdbFailure::Quota => "the browser's storage for this site is full".to_string(),
    IdbFailure::Other(reason) => reason,
  };
  SaveError::Io {
    path: name.to_string(),
    source: std::io::Error::other(reason),
  }
}

fn failure(error: &JsValue) -> IdbFailure {
  match error.dyn_ref::<DomException>() {
    Some(exception) => dom_failure(exception),
    None => IdbFailure::Other(format!("{error:?}")),
  }
}

fn dom_failure(exception: &DomException) -> IdbFailure {
  if exception.name() == "QuotaExceededError" {
    IdbFailure::Quota
  } else {
    IdbFailure::Other(format!("{}: {}", exception.name(), exception.message()))
  }
}
