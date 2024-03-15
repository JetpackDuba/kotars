mod functions;
mod structs;
mod types_transformations;

extern crate proc_macro;

use proc_macro::TokenStream;
use structs::JniGenerator;

use crate::functions::generate_rust_functions;
use crate::structs::{Class, DataClass, FromSyn};
use kotars_common::{Function, JniType, Parameter, RsStruct};
use quote::{quote, ToTokens};
use syn::__private::{str, TokenStream2};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{parse_macro_input, FnArg, ImplItem, ItemImpl, ItemStruct, LitStr, ReturnType};

pub(crate) const AUTO_GENERATED_HEADER_TEXT: &str = "Auto generated header. This will be used by cargo-kotars to generate the Kotlin code that binds to the Rust code.";

#[proc_macro]
pub fn jni_init(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);

    let package_name = quote! { #input };
    println!("Package name: {package_name}");

    // todo move IntoEnv interface as part of the kotars crate instead of being generated
    let base_definition = quote! {
        pub const JNI_PACKAGE_NAME: &str = #package_name;

        trait IntoEnv<'a, T> {
            fn into_env(self, env: &mut jni::JNIEnv<'a>) -> T;
        }

        impl <'local> IntoEnv<'local, jni::objects::JString<'local>> for String {
            fn into_env(self, env: &mut jni::JNIEnv<'local>) -> jni::objects::JString<'local> {
                env
                    .new_string(self)
                    .expect("Couldn't create java string!")
            }
        }

        impl IntoEnv<'_, String> for jni::objects::JString<'_> {
            fn into_env(self, env: &mut jni::JNIEnv) -> String {
                env
                    .get_string(&self)
                    .expect("Couldn't get java string!")
                    .into()
            }
        }

        impl IntoEnv<'_, Vec<u8>> for jni::objects::JByteArray<'_> {
            fn into_env(self, env: &mut jni::JNIEnv) -> Vec<u8> {
                env.convert_byte_array(self).unwrap()
            }
        }
        impl <'local> IntoEnv<'local, jni::objects::JByteArray<'local>> for Vec<u8> {
            fn into_env(self, env: &mut jni::JNIEnv<'local>) -> jni::objects::JByteArray<'local> {
                let output = env.byte_array_from_slice(&self).unwrap();
                output
            }
        }
    };

    base_definition.into()
}

#[proc_macro_attribute]
pub fn jni_struct_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_impl = parse_macro_input!(item as ItemImpl);
    let struct_name = input_impl.self_ty.as_ref().to_token_stream().to_string();

    let functions = input_impl
        .items
        .iter()
        .filter_map(|item| {
            if let ImplItem::Fn(method) = item {
                let method_name = &method.sig.ident;
                let parameters = get_parameters_from_method(&method.sig.inputs);
                let return_type = get_return_type_from_method(&method.sig.output);

                Some(Function {
                    owner_name: struct_name.clone(),
                    name: method_name.to_string(),
                    parameters,
                    return_type,
                })
            } else {
                None
            }
        })
        .collect::<Vec<Function>>();

    let new_functions = generate_rust_functions(&struct_name, &functions);

    let output = quote! {
        #input_impl
        #(#new_functions)*
    };

    output.into()
}

#[proc_macro_attribute]
pub fn jni_class(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let item_struct = parse_macro_input!(input as ItemStruct);

    let rs_struct = RsStruct::from_syn(item_struct.clone());
    let class: Class = rs_struct.into();

    jni_class_generator(item_struct, &class)
}

#[proc_macro_attribute]
pub fn jni_data_class(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let item_struct = parse_macro_input!(input as ItemStruct);

    let rs_struct = RsStruct::from_syn(item_struct.clone());
    let data_class: DataClass = rs_struct.into();

    jni_class_generator(item_struct, &data_class)
}

fn jni_class_generator(item_struct: ItemStruct, jni_generator: &impl JniGenerator) -> TokenStream {
    let methods = jni_generator.generated_methods();

    let out = quote! {
        #item_struct

        #(#methods)*
    };

    out.into()
}

pub(crate) fn full_header_comment(content: &str) -> TokenStream2 {
    let header_comments = format!(
        r#"
        /// JNI_BINDING_START
        /// {content}
        /// JNI_BINDING_END
        /// {AUTO_GENERATED_HEADER_TEXT}
        "#
    )
    .trim_start()
    .to_string();

    syn::parse_str(&header_comments).unwrap()

}

#[proc_macro_attribute]
pub fn jni_interface(_attr: TokenStream, _input: TokenStream) -> TokenStream {
    todo!()
}

fn rust_property_to_jni_type(
    ty: &JniType,
    param: &TokenStream2,
    struct_parameter: &TokenStream2,
) -> TokenStream2 {
    match ty {
        JniType::Int32 => {
            quote! {
                let #param = #struct_parameter as jni::sys::jint;
            }
        }
        JniType::Int64 => {
            quote! {
                let #param = #struct_parameter as jni::sys::jlong;
            }
        }
        JniType::String => {
            quote! {
                let #param: jni::objects::JString = #struct_parameter.into_env(env);
                let #param: jni::objects::JObject = #param.into();
                let #param: jni::objects::JValue = jni::objects::JValue::Object(&#param);
            }
        }
        JniType::Boolean => {
            quote! {
                let #param = #struct_parameter as jni::sys::jboolean;
            }
        }
        JniType::Receiver(_) | JniType::CustomType(_) => {
            quote! {
                let #param: jni::objects::JObject = #struct_parameter.into_env(env);
                let #param: jni::objects::JValue = jni::objects::JValue::Object(&#param);
            }
        }
    }
}

fn get_return_type_from_method(return_type: &ReturnType) -> Option<JniType> {
    match return_type {
        ReturnType::Default => None,
        ReturnType::Type(_token, typ) => {
            let rt: JniType = quote::quote!(#typ).to_string().into();

            Some(rt)
        }
    }
}

fn get_parameters_from_method(inputs: &Punctuated<FnArg, Comma>) -> Vec<Parameter> {
    inputs
        .iter()
        .map(|param| {
            match param {
                FnArg::Receiver(rec) => {
                    if rec.reference.is_none() {
                        // TODO check what would happen if the memory is freed while the JVM still points to it
                        panic!("You must not take ownership of `self` to prevent crashes in the JVM after freeing the memory")
                    }

                    Parameter::Receiver {
                        is_mutable: rec.mutability.is_some(),
                    }
                }
                FnArg::Typed(pat_type) => {
                    let pat = &pat_type.pat;
                    let ty = &pat_type.ty;

                    Parameter::Typed {
                        name: quote! {#pat}.to_string(),
                        ty: quote! {#ty}.to_string().into(),
                    }
                }
            }
        })
        .collect()
}
