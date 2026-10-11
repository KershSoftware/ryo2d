//! Netplay testing command
//!
//! Starts a local matchbox signaling server and exposes it via Cloudflare Tunnel
//! for testing multiplayer with friends.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use super::{CliError, CliResult, Output};

/// Execute the netplay command
pub fn execute(args: &[String]) -> CliResult<()> {
  if args.is_empty() {
    print_help();
    return Ok(());
  }

  match args[0].as_str() {
    "start" => execute_start(&args[1..]),
    "ip" => execute_ip(),
    "help" | "--help" | "-h" => {
      print_help();
      Ok(())
    }
    unknown => {
      Output::error(&format!("Unknown subcommand: {}", unknown));
      println!();
      print_help();
      Err(CliError::Other(format!("Unknown subcommand: {}", unknown)))
    }
  }
}

fn print_help() {
  println!(
    r#"
ryo2d netplay - Tools for testing multiplayer/netplay

USAGE:
    ryo2d netplay <COMMAND>

COMMANDS:
    start       Start a temporary signaling server for testing with friends
    ip          Show your LAN and WAN IP addresses
    help        Show this help message

NOTE:
    'netplay start' creates a temporary tunnel that closes when you stop it.
    For production games, deploy your own signaling server (see docs/).

EXAMPLES:
    ryo2d netplay start
    ryo2d netplay ip

Run 'ryo2d netplay <COMMAND> --help' for more information.
"#
  );
}

// =============================================================================
// netplay start
// =============================================================================

fn execute_start(args: &[String]) -> CliResult<()> {
  for arg in args {
    if arg == "--help" || arg == "-h" {
      print_start_help();
      return Ok(());
    }
  }

  run_netplay_server()
}

fn print_start_help() {
  println!(
    r#"
ryo2d netplay start - Start a TEMPORARY signaling server for testing

USAGE:
    ryo2d netplay start

DESCRIPTION:
    Creates a temporary tunnel for testing multiplayer with friends over the
    internet. The tunnel URL changes each time and closes when you press Ctrl+C.

    NOT FOR PRODUCTION - For shipped games, deploy your own signaling server.
    See: docs/architecture/matchbox-server.md

    Press Ctrl+C to stop the server and close the tunnel.

REQUIREMENTS:
    - matchbox_server (auto-installed via cargo)
    - cloudflared: https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/install-and-setup/installation/

      Windows:   winget install Cloudflare.cloudflared
      macOS:     brew install cloudflared
      Linux:     See Cloudflare docs for your distro
"#
  );
}

fn run_netplay_server() -> CliResult<()> {
  let port = 3536u16;
  Output::status("Netplay", "Starting signaling server for testing...");

  // Check requirements and show install instructions if missing
  let matchbox_path = find_command("matchbox_server");
  let mut cloudflared_path = find_command("cloudflared");

  if matchbox_path.is_none() || cloudflared_path.is_none() {
    println!();
    println!("  Missing requirements:");
    println!();

    if matchbox_path.is_none() {
      println!("  [ ] matchbox_server");
      println!("      Install: cargo install matchbox_server");
      println!();
    }

    if cloudflared_path.is_none() {
      println!("  [ ] cloudflared");
      #[cfg(windows)]
      println!("      Install: winget install Cloudflare.cloudflared");
      #[cfg(target_os = "macos")]
      println!("      Install: brew install cloudflared");
      #[cfg(target_os = "linux")]
      println!(
        "      Install: See https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/install-and-setup/installation/"
      );
      println!();
    }

    // Offer to install matchbox_server automatically
    if matchbox_path.is_none() {
      Output::status("Installing", "matchbox_server (this may take a minute)...");
      let status = Command::new("cargo")
        .args(["install", "matchbox_server"])
        .status()
        .map_err(|e| CliError::CommandFailed {
          command: "cargo install".to_string(),
          message: e.to_string(),
        })?;

      if !status.success() {
        return Err(CliError::CommandFailed {
          command: "cargo install matchbox_server".to_string(),
          message: "Installation failed".to_string(),
        });
      }
    }

    // Offer to install cloudflared automatically
    if cloudflared_path.is_none() {
      if prompt_yes_no("Install cloudflared automatically?") {
        if install_cloudflared() {
          // Re-check for cloudflared after installation
          cloudflared_path = find_command("cloudflared");
          if cloudflared_path.is_none() {
            return Err(CliError::Other(
              "cloudflared installed but not found. Try restarting your terminal.".to_string(),
            ));
          }
        } else {
          return Err(CliError::Other(
            "Failed to install cloudflared. Please install manually and try again.".to_string(),
          ));
        }
      } else {
        return Err(CliError::Other(
          "cloudflared is required. Please install it and try again.".to_string(),
        ));
      }
    }
  }

  // Get the actual paths (we know they exist at this point)
  let matchbox_cmd = matchbox_path.unwrap_or_else(|| "matchbox_server".to_string());
  let cloudflared_cmd = cloudflared_path.unwrap_or_else(|| "cloudflared".to_string());

  // Start matchbox_server
  Output::status("Starting", &format!("matchbox_server on port {}...", port));

  let mut matchbox = Command::new(&matchbox_cmd)
    .env("RUST_LOG", "warn")
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| CliError::CommandFailed {
      command: matchbox_cmd.clone(),
      message: e.to_string(),
    })?;

  // Give it a moment to start
  thread::sleep(Duration::from_millis(500));

  // Check if it's still running
  match matchbox.try_wait() {
    Ok(Some(status)) => {
      // Read stderr to get the actual error message
      let mut detail = String::new();
      if let Some(stderr) = matchbox.stderr.take() {
        let mut reader = BufReader::new(stderr);
        let _ = reader.read_line(&mut detail);
      }
      let detail = detail.trim();

      let message = if detail.contains("AddrInUse") || detail.contains("already in use") {
        format!(
          "Port {} is already in use. A matchbox_server may still be running from a previous session.\n\
           Try: taskkill /F /IM matchbox_server.exe (Windows) or killall matchbox_server (Unix)",
          port
        )
      } else if detail.is_empty() {
        format!("Exited immediately with status: {}", status)
      } else {
        format!("Exited immediately: {}", detail)
      };

      return Err(CliError::CommandFailed {
        command: matchbox_cmd,
        message,
      });
    }
    Ok(None) => {} // Still running, good
    Err(e) => {
      return Err(CliError::CommandFailed {
        command: matchbox_cmd,
        message: e.to_string(),
      });
    }
  }

  // Start cloudflared tunnel
  Output::status("Starting", "Cloudflare tunnel...");

  let mut cloudflared = Command::new(&cloudflared_cmd)
    .args(["tunnel", "--url", &format!("http://localhost:{}", port)])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| CliError::CommandFailed {
      command: cloudflared_cmd.clone(),
      message: e.to_string(),
    })?;

  // Set up running flag for shutdown coordination
  let running = Arc::new(AtomicBool::new(true));

  // Read cloudflared output to find the tunnel URL
  let stderr = cloudflared.stderr.take().unwrap();
  let reader = BufReader::new(stderr);

  let mut tunnel_url: Option<String> = None;

  // Spawn a thread to read cloudflared output
  let running_clone = running.clone();
  let output_thread = thread::spawn(move || {
    for line in reader.lines() {
      if !running_clone.load(Ordering::SeqCst) {
        break;
      }

      // Look for the tunnel URL in the output
      if let Ok(line) = line
        && line.contains("trycloudflare.com")
        && let Some(url) = extract_url(&line)
      {
        return Some(url);
      }
    }
    None
  });

  // Wait for URL with timeout
  let start = std::time::Instant::now();
  while tunnel_url.is_none() && start.elapsed() < Duration::from_secs(30) {
    if !running.load(Ordering::SeqCst) {
      break;
    }

    if output_thread.is_finished() {
      tunnel_url = output_thread.join().ok().flatten();
      break;
    }

    thread::sleep(Duration::from_millis(100));
  }

  if let Some(url) = tunnel_url {
    // Convert http to wss for WebSocket
    let ws_url = url
      .replace("https://", "wss://")
      .replace("http://", "ws://");

    println!();
    println!(
      "  ╔════════════════════════════════════════════════════════════════════════════════╗"
    );
    println!(
      "  ║                      NETPLAY TEST SERVER READY (temporary)                     ║"
    );
    println!(
      "  ╠════════════════════════════════════════════════════════════════════════════════╣"
    );
    println!(
      "  ║                                                                                ║"
    );
    println!(
      "  ║  Share this URL with friends:                                                  ║"
    );
    println!(
      "  ║                                                                                ║"
    );
    println!("  ║    {:<75} ║", ws_url);
    println!(
      "  ║                                                                                ║"
    );
    println!(
      "  ╠════════════════════════════════════════════════════════════════════════════════╣"
    );
    println!(
      "  ║  Ctrl+C to stop (tunnel closes, URL stops working)                             ║"
    );
    println!(
      "  ╚════════════════════════════════════════════════════════════════════════════════╝"
    );
    println!();

    // Wait for Ctrl+C (process termination)
    loop {
      thread::sleep(Duration::from_millis(500));
      // Check if cloudflared is still running
      match cloudflared.try_wait() {
        Ok(Some(_)) => break, // Process exited
        Ok(None) => {}        // Still running
        Err(_) => break,
      }
    }
  } else {
    Output::warning("Could not detect tunnel URL. Check cloudflared output.");
    println!("  The server is running. Look for the URL in the output above.");
    println!("  Press Ctrl+C to stop.");

    loop {
      thread::sleep(Duration::from_millis(500));
      match cloudflared.try_wait() {
        Ok(Some(_)) => break,
        Ok(None) => {}
        Err(_) => break,
      }
    }
  }

  // Cleanup
  Output::status("Stopping", "servers...");
  let _ = cloudflared.kill();
  let _ = matchbox.kill();
  Output::success("Netplay server stopped");

  Ok(())
}

fn extract_url(line: &str) -> Option<String> {
  // Look for https://...trycloudflare.com pattern
  if let Some(start) = line.find("https://") {
    let rest = &line[start..];
    let end = rest
      .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '>')
      .unwrap_or(rest.len());
    let url = &rest[..end];
    if url.contains("trycloudflare.com") {
      return Some(url.to_string());
    }
  }
  None
}

// =============================================================================
// netplay ip
// =============================================================================

fn execute_ip() -> CliResult<()> {
  println!();
  println!("  Network Addresses");
  println!("  =================");
  println!();

  // Get LAN IPs
  let lan_ips = get_lan_ips();
  if lan_ips.is_empty() {
    println!("  LAN:  (could not detect)");
  } else {
    for (i, ip) in lan_ips.iter().enumerate() {
      if i == 0 {
        println!("  LAN:  {}", ip);
      } else {
        println!("        {}", ip);
      }
    }
  }

  println!();

  // Get WAN IP
  Output::status("Fetching", "public IP...");
  match get_wan_ip() {
    Some(ip) => println!("  WAN:  {}", ip),
    None => println!("  WAN:  (could not detect - check internet connection)"),
  }

  println!();
  println!("  ┌─────────────────────────────────────────────────────────────────┐");
  println!("  │ LAN play (same network)                                         │");
  println!("  │   Friends connect to your LAN IP using UDP. No setup needed.    │");
  println!("  │                                                                 │");
  println!("  │ Internet play                                                   │");
  println!("  │   Option A: Port forward UDP on your router, share your WAN IP  │");
  println!("  │             (complex, requires router access)                   │");
  println!("  │                                                                 │");
  println!("  │   Option B: Run 'ryo2d netplay start' for a temporary tunnel    │");
  println!("  │             (easy, no port forwarding, closes when you stop it) │");
  println!("  └─────────────────────────────────────────────────────────────────┘");
  println!();

  Ok(())
}

fn get_lan_ips() -> Vec<String> {
  let mut ips = Vec::new();

  // Try to get local IPs using platform-specific commands
  #[cfg(windows)]
  {
    // Use -NoProfile to avoid shell welcome messages bleeding into output
    if let Ok(output) = Command::new("powershell")
      .args([
        "-NoProfile",
        "-Command",
        "(Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.InterfaceAlias -notlike '*Loopback*' -and $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' }).IPAddress",
      ])
      .output()
      && output.status.success()
    {
      let stdout = String::from_utf8_lossy(&output.stdout);
      for line in stdout.lines() {
        let ip = line.trim();
        // Only accept lines that look like IPv4 addresses
        if is_valid_ipv4(ip) {
          ips.push(ip.to_string());
        }
      }
    }
  }

  #[cfg(unix)]
  {
    // Try hostname -I first (Linux)
    if let Ok(output) = Command::new("hostname").args(["-I"]).output() {
      if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for ip in stdout.split_whitespace() {
          if is_valid_ipv4(ip) && !ip.starts_with("127.") {
            ips.push(ip.to_string());
          }
        }
      }
    }

    // Fallback to ifconfig/ip
    if ips.is_empty() {
      if let Ok(output) = Command::new("ip").args(["addr"]).output() {
        if output.status.success() {
          let stdout = String::from_utf8_lossy(&output.stdout);
          for line in stdout.lines() {
            if line.contains("inet ") && !line.contains("127.0.0.1") {
              if let Some(ip) = line.split_whitespace().nth(1) {
                if let Some(ip) = ip.split('/').next() {
                  if is_valid_ipv4(ip) {
                    ips.push(ip.to_string());
                  }
                }
              }
            }
          }
        }
      }
    }
  }

  ips
}

/// Check if a string looks like a valid IPv4 address
fn is_valid_ipv4(s: &str) -> bool {
  let parts: Vec<&str> = s.split('.').collect();
  if parts.len() != 4 {
    return false;
  }
  parts.iter().all(|part| {
    !part.is_empty()
      && part.len() <= 3
      && part.chars().all(|c| c.is_ascii_digit())
      && part.parse::<u8>().is_ok()
  })
}

fn get_wan_ip() -> Option<String> {
  // Try multiple services in case one is down
  let services = [
    (
      "curl",
      vec!["curl", "-s", "-m", "5", "https://api.ipify.org"],
    ),
    (
      "curl",
      vec!["curl", "-s", "-m", "5", "https://icanhazip.com"],
    ),
    ("curl", vec!["curl", "-s", "-m", "5", "https://ifconfig.me"]),
  ];

  for (cmd, args) in services {
    if let Ok(output) = Command::new(cmd).args(&args[1..]).output()
      && output.status.success()
    {
      let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
      // Basic IP validation
      if ip.len() >= 7 && ip.len() <= 15 && ip.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Some(ip);
      }
    }
  }

  // Windows fallback using PowerShell
  #[cfg(windows)]
  {
    if let Ok(output) = Command::new("powershell")
      .args([
        "-Command",
        "(Invoke-WebRequest -Uri 'https://api.ipify.org' -UseBasicParsing -TimeoutSec 5).Content",
      ])
      .output()
      && output.status.success()
    {
      let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
      if ip.len() >= 7 && ip.len() <= 15 && ip.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Some(ip);
      }
    }
  }

  None
}

fn command_exists(cmd: &str) -> bool {
  find_command(cmd).is_some()
}

/// Find a command, returning its full path if found
fn find_command(cmd: &str) -> Option<String> {
  #[cfg(windows)]
  {
    // First check if it's in PATH
    if let Ok(output) = Command::new("where").arg(cmd).output()
      && output.status.success()
    {
      let path = String::from_utf8_lossy(&output.stdout);
      if let Some(first_line) = path.lines().next() {
        let trimmed = first_line.trim();
        if !trimmed.is_empty() {
          return Some(trimmed.to_string());
        }
      }
    }

    // Check common install locations for cloudflared
    if cmd == "cloudflared" {
      let common_paths = [
        r"C:\Program Files (x86)\cloudflared\cloudflared.exe",
        r"C:\Program Files\cloudflared\cloudflared.exe",
      ];

      for path in common_paths {
        if std::path::Path::new(path).exists() {
          return Some(path.to_string());
        }
      }

      // Check winget packages directory
      if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        let winget_dir = format!(r"{}\Microsoft\WinGet\Packages", localappdata);
        if let Ok(entries) = std::fs::read_dir(&winget_dir) {
          for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
              && let Some(name) = path.file_name().and_then(|n| n.to_str())
              && name.starts_with("Cloudflare.cloudflared")
            {
              let exe_path = path.join("cloudflared.exe");
              if exe_path.exists() {
                return Some(exe_path.to_string_lossy().to_string());
              }
            }
          }
        }
      }
    }

    None
  }

  #[cfg(not(windows))]
  {
    Command::new("which")
      .arg(cmd)
      .output()
      .ok()
      .filter(|o| o.status.success())
      .and_then(|o| {
        String::from_utf8_lossy(&o.stdout)
          .lines()
          .next()
          .map(|s| s.trim().to_string())
      })
      .filter(|s| !s.is_empty())
  }
}

/// Prompt the user for a yes/no answer
fn prompt_yes_no(question: &str) -> bool {
  use std::io::{self, Write};

  print!("  {} [Y/n] ", question);
  io::stdout().flush().unwrap();

  let mut input = String::new();
  if io::stdin().read_line(&mut input).is_err() {
    return false;
  }

  let input = input.trim().to_lowercase();
  input.is_empty() || input == "y" || input == "yes"
}

/// Attempt to install cloudflared automatically
fn install_cloudflared() -> bool {
  Output::status("Installing", "cloudflared...");

  #[cfg(windows)]
  {
    // Try winget first
    if command_exists("winget") {
      let status = Command::new("winget")
        .args([
          "install",
          "--id",
          "Cloudflare.cloudflared",
          "--accept-source-agreements",
          "--accept-package-agreements",
        ])
        .status();

      if status.map(|s| s.success()).unwrap_or(false) {
        Output::success("cloudflared installed via winget");
        println!();
        println!("  NOTE: You may need to restart your terminal for cloudflared to be in PATH.");
        println!();
        return true;
      }
    }

    Output::error("Could not install cloudflared automatically.");
    println!("  Please install manually: winget install Cloudflare.cloudflared");
    false
  }

  #[cfg(target_os = "macos")]
  {
    // Try brew
    if command_exists("brew") {
      let status = Command::new("brew")
        .args(["install", "cloudflared"])
        .status();

      if status.map(|s| s.success()).unwrap_or(false) {
        Output::success("cloudflared installed via brew");
        return true;
      }
    }

    Output::error("Could not install cloudflared automatically.");
    println!("  Please install manually: brew install cloudflared");
    false
  }

  #[cfg(target_os = "linux")]
  {
    // Linux is more complex, just point to docs
    Output::error("Automatic installation not supported on Linux.");
    println!("  Please install manually:");
    println!(
      "  https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/install-and-setup/installation/"
    );
    false
  }

  #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
  {
    Output::error("Automatic installation not supported on this platform.");
    false
  }
}
