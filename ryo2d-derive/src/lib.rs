//! Derive macros for ryo2d game engine
//!
//! This crate provides procedural macros for the ryo2d game engine:
//!
//! - [`NetworkInput`] - Generates a builder with required fields for network input structs

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Ident, Type, parse_macro_input};

/// Derive macro for network input structs with compile-time required field checking.
///
/// This macro generates a builder pattern that enforces required fields at compile time
/// using the typestate pattern. Fields marked with `#[network_input(required)]` must
/// be set before `build()` can be called.
///
/// # Example
///
/// ```ignore
/// use ryo2d_derive::NetworkInput;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(NetworkInput, Clone, Default, Serialize, Deserialize)]
/// pub struct PlayerInput {
///     #[network_input(required)]
///     pub move_x: f32,
///     #[network_input(required)]
///     pub move_y: f32,
///     #[network_input(required)]
///     pub facing_right: bool,
///     pub jump: bool,        // Optional, uses default
///     pub attack: bool,      // Optional, uses default
/// }
///
/// // Usage - compiler enforces required fields
/// let input = PlayerInput::builder()
///     .move_x(1.0)
///     .move_y(0.0)
///     .facing_right(true)
///     .jump(true)           // Optional
///     .build();             // Only available when all required fields set
///
/// // This won't compile - missing facing_right:
/// // let input = PlayerInput::builder()
/// //     .move_x(1.0)
/// //     .move_y(0.0)
/// //     .build();  // ERROR: build() not available
/// ```
///
/// # Generated Code
///
/// For each struct, the macro generates:
/// - Marker types for typestate (`Required0Set`, `Required0Unset`, etc.)
/// - A builder struct with generic type parameters for each required field
/// - Setter methods that transition the typestate
/// - A `build()` method only available when all required fields are `Set`
///
/// # Attributes
///
/// - `#[network_input(required)]` - Mark a field as required (must be set before build)
#[proc_macro_derive(NetworkInput, attributes(network_input))]
pub fn derive_network_input(input: TokenStream) -> TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  match impl_network_input(&input) {
    Ok(tokens) => tokens.into(),
    Err(err) => err.to_compile_error().into(),
  }
}

fn impl_network_input(input: &DeriveInput) -> syn::Result<TokenStream2> {
  let struct_name = &input.ident;
  let builder_name = format_ident!("{}Builder", struct_name);
  let vis = &input.vis;

  let fields = match &input.data {
    Data::Struct(data) => match &data.fields {
      Fields::Named(fields) => &fields.named,
      _ => {
        return Err(syn::Error::new_spanned(
          input,
          "NetworkInput only supports structs with named fields",
        ));
      }
    },
    _ => {
      return Err(syn::Error::new_spanned(
        input,
        "NetworkInput only supports structs",
      ));
    }
  };

  // Collect field info
  let mut all_fields: Vec<FieldInfo> = Vec::new();
  let mut required_indices: Vec<usize> = Vec::new();

  for field in fields.iter() {
    let field_name = field.ident.as_ref().unwrap();
    let field_type = &field.ty;

    let is_required = field.attrs.iter().any(|attr| {
      if attr.path().is_ident("network_input") {
        attr
          .parse_nested_meta(|meta| {
            if meta.path.is_ident("required") {
              Ok(())
            } else {
              Err(meta.error("expected `required`"))
            }
          })
          .is_ok()
      } else {
        false
      }
    });

    if is_required {
      required_indices.push(all_fields.len());
    }

    all_fields.push(FieldInfo {
      name: field_name.clone(),
      ty: field_type.clone(),
      is_required,
    });
  }

  let num_required = required_indices.len();

  // Generate typestate marker types (unique per struct)
  let set_markers: Vec<Ident> = (0..num_required)
    .map(|i| format_ident!("__{}Required{}Set", struct_name, i))
    .collect();
  let unset_markers: Vec<Ident> = (0..num_required)
    .map(|i| format_ident!("__{}Required{}Unset", struct_name, i))
    .collect();

  let marker_defs = set_markers
    .iter()
    .zip(unset_markers.iter())
    .map(|(set, unset)| {
      quote! {
          #[doc(hidden)]
          #vis struct #set;
          #[doc(hidden)]
          #vis struct #unset;
      }
    });

  // Generate type parameters for the builder
  let type_params: Vec<Ident> = (0..num_required).map(|i| format_ident!("R{}", i)).collect();

  let type_params_decl = if type_params.is_empty() {
    quote! {}
  } else {
    quote! { < #(#type_params),* > }
  };

  // Generate builder struct fields
  let builder_fields: Vec<_> = all_fields
    .iter()
    .map(|f| {
      let name = &f.name;
      let ty = &f.ty;
      quote! { #name: Option<#ty> }
    })
    .collect();

  // Generate phantom data for type parameters
  let phantom_fields: Vec<_> = type_params
    .iter()
    .enumerate()
    .map(|(i, tp)| {
      let phantom_name = format_ident!("_phantom{}", i);
      quote! { #phantom_name: std::marker::PhantomData<#tp> }
    })
    .collect();

  // Generate the builder struct
  let builder_struct = if type_params.is_empty() {
    quote! {
        #[doc(hidden)]
        #vis struct #builder_name {
            #(#builder_fields,)*
        }
    }
  } else {
    quote! {
        #[doc(hidden)]
        #vis struct #builder_name #type_params_decl {
            #(#builder_fields,)*
            #(#phantom_fields,)*
        }
    }
  };

  // Generate the initial builder (all Unset)
  let initial_builder_type = if type_params.is_empty() {
    quote! { #builder_name }
  } else {
    quote! { #builder_name < #(#unset_markers),* > }
  };

  let initial_phantom_inits: Vec<_> = (0..num_required)
    .map(|i| {
      let phantom_name = format_ident!("_phantom{}", i);
      quote! { #phantom_name: std::marker::PhantomData }
    })
    .collect();

  let field_inits: Vec<_> = all_fields
    .iter()
    .map(|f| {
      let name = &f.name;
      quote! { #name: None }
    })
    .collect();

  let builder_init = if type_params.is_empty() {
    quote! {
        #builder_name {
            #(#field_inits,)*
        }
    }
  } else {
    quote! {
        #builder_name {
            #(#field_inits,)*
            #(#initial_phantom_inits,)*
        }
    }
  };

  // Generate setter methods
  let mut setter_methods = Vec::new();
  let mut required_idx = 0;

  for field in &all_fields {
    let field_name = &field.name;
    let field_type = &field.ty;

    if field.is_required {
      // Required field: transitions typestate from Unset to Set
      let new_type_params: Vec<_> = type_params
        .iter()
        .enumerate()
        .map(|(i, _)| {
          if i == required_idx {
            let set = &set_markers[i];
            quote! { #set }
          } else {
            let tp = &type_params[i];
            quote! { #tp }
          }
        })
        .collect();

      let new_phantoms: Vec<_> = (0..num_required)
        .map(|i| {
          let phantom_name = format_ident!("_phantom{}", i);
          quote! { #phantom_name: std::marker::PhantomData }
        })
        .collect();

      let other_field_copies: Vec<_> = all_fields
        .iter()
        .filter(|f| f.name != *field_name)
        .map(|f| {
          let n = &f.name;
          quote! { #n: self.#n }
        })
        .collect();

      if type_params.is_empty() {
        setter_methods.push(quote! {
            #vis fn #field_name(mut self, value: #field_type) -> #builder_name {
                self.#field_name = Some(value);
                self
            }
        });
      } else {
        setter_methods.push(quote! {
                    #vis fn #field_name(self, value: #field_type) -> #builder_name < #(#new_type_params),* > {
                        #builder_name {
                            #field_name: Some(value),
                            #(#other_field_copies,)*
                            #(#new_phantoms,)*
                        }
                    }
                });
      }

      required_idx += 1;
    } else {
      // Optional field: doesn't change typestate
      setter_methods.push(quote! {
          #vis fn #field_name(mut self, value: #field_type) -> Self {
              self.#field_name = Some(value);
              self
          }
      });
    }
  }

  // Generate impl blocks for setters
  // We need one impl block that covers all possible typestate combinations
  let setter_impl = if type_params.is_empty() {
    quote! {
        impl #builder_name {
            #(#setter_methods)*
        }
    }
  } else {
    quote! {
        impl < #(#type_params),* > #builder_name < #(#type_params),* > {
            #(#setter_methods)*
        }
    }
  };

  // Generate build() method - only available when all required fields are Set
  let all_set_type = if type_params.is_empty() {
    quote! { #builder_name }
  } else {
    quote! { #builder_name < #(#set_markers),* > }
  };

  let build_field_assigns: Vec<_> = all_fields
    .iter()
    .map(|f| {
      let name = &f.name;
      if f.is_required {
        quote! {
            #name: self.#name.expect("required field not set")
        }
      } else {
        quote! {
            #name: self.#name.unwrap_or_default()
        }
      }
    })
    .collect();

  let build_impl = quote! {
      impl #all_set_type {
          /// Build the input struct.
          ///
          /// This method is only available when all required fields have been set.
          #vis fn build(self) -> #struct_name {
              #struct_name {
                  #(#build_field_assigns,)*
              }
          }
      }
  };

  // Generate builder() associated function
  let builder_fn = quote! {
      impl #struct_name {
          /// Create a new builder for this input type.
          ///
          /// Required fields must be set before `build()` can be called.
          #vis fn builder() -> #initial_builder_type {
              #builder_init
          }
      }
  };

  // Combine all generated code
  let expanded = quote! {
      #(#marker_defs)*

      #builder_struct

      #setter_impl

      #build_impl

      #builder_fn
  };

  Ok(expanded)
}

struct FieldInfo {
  name: Ident,
  ty: Type,
  is_required: bool,
}
