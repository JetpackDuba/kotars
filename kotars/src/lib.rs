extern crate proc_macro;

use proc_macro::TokenStream;

use quote::{quote, ToTokens};
use syn::{FnArg, ImplItem, ItemImpl, ItemStruct, ItemTrait, LitStr, parse_macro_input, ReturnType, TraitItem};
use syn::__private::{str, TokenStream2};
use syn::punctuated::Punctuated;
use syn::token::Comma;

use kotars_common::{Field, Function, JniType, Parameter, RsInterface, RsStruct, string_to_camel_case};
use structs::JniGenerator;

use crate::functions::generate_rust_jni_binding_functions;
use crate::structs::{Class, DataClass, FromSyn};

mod functions;
mod structs;
mod types_transformations;

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
            fn into_env(self, env: &mut std::cell::RefMut<'_, jni::JNIEnv<'a>>) -> T;
        }

        impl <'local> IntoEnv<'local, jni::objects::JString<'local>> for String {
            fn into_env(self, env: &mut std::cell::RefMut<'_, jni::JNIEnv<'local>>) -> jni::objects::JString<'local> {
                env
                    .new_string(self)
                    .expect("Couldn't create java string!")
            }
        }

        impl IntoEnv<'_, String> for jni::objects::JString<'_> {
            fn into_env(self, env: &mut std::cell::RefMut<'_, jni::JNIEnv>) -> String {
                env
                    .get_string(&self)
                    .expect("Couldn't get java string!")
                    .into()
            }
        }

        impl IntoEnv<'_, Vec<u8>> for jni::objects::JByteArray<'_> {
            fn into_env(self, env: &mut std::cell::RefMut<'_, jni::JNIEnv>) -> Vec<u8> {
                env.convert_byte_array(self).unwrap()
            }
        }
        impl <'local> IntoEnv<'local, jni::objects::JByteArray<'local>> for Vec<u8> {
            fn into_env(self, env: &mut std::cell::RefMut<'_, jni::JNIEnv<'local>>) -> jni::objects::JByteArray<'local> {
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

    let new_functions = generate_rust_jni_binding_functions(&struct_name, &functions);

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
pub fn jni_interface(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let item_trait = parse_macro_input!(input as ItemTrait);
    let trait_name = item_trait.ident.to_string();
    let trait_token = &item_trait.ident;

    let trait_implementer_name = format!("{trait_name}JniBridge");
    let trait_implementer_name = syn::parse_str::<TokenStream2>(&trait_implementer_name).unwrap();

    let functions = item_trait
        .items
        .iter()
        .filter_map(|item| {
            if let TraitItem::Fn(func) = item {
                let method_name = &func.sig.ident;
                let return_type = &func.sig.output;

                let inputs = &func.sig.inputs;
                let str_method_name = string_to_camel_case(method_name.to_string().as_str());

                let return_type_signature = match return_type {
                    ReturnType::Default => { structs::jni_type_to_jni_method_signature_type(&JniType::Void) }
                    ReturnType::Type(_, ty) => {
                        let jni_ty = quote::quote!(#ty).to_string().into();
                        structs::jni_type_to_jni_method_signature_type(&jni_ty)
                    }
                };

                let method_types_signature = &func.sig.inputs.iter()
                    .filter_map(|field| {
                        match field {
                            FnArg::Receiver(_) => {
                                None
                            }
                            FnArg::Typed(pat_ty) => {
                                let ty = &pat_ty.ty;
                                let jni_ty = quote::quote!(#ty).to_string().into();
                                Some(structs::jni_type_to_jni_method_signature_type(&jni_ty))
                            }
                        }
                    })
                    .collect::<Vec<String>>()
                    .join("");

                let method_types_signature = format!("({method_types_signature}){return_type_signature}");
                let fields = func
                    .sig
                    .inputs
                    .iter()
                    .filter_map(|field| {
                        match field {
                            FnArg::Receiver(_) => { None }
                            FnArg::Typed(pat_ty) => {
                                let pat = &pat_ty.pat;
                                let name = quote! { #pat }.to_string();
                                let original_ty = &pat_ty.ty;
                                let ty = quote! { #original_ty }.to_string();
                                let jni_ty: JniType = ty.into();

                                let field = Field {
                                    is_public: true,
                                    name: Some(name),
                                    ty: jni_ty,
                                };

                                Some(field)
                            }
                        }
                    })
                    .collect::<Vec<Field>>();

                let transformations = structs::generate_method_fields_transformation(&fields);
                let params_into_array = structs::generate_struct_fields_mapping_into_array(&fields);
                
                let error_msg = format!("Call method [{str_method_name}] with signature [{method_types_signature}] failed with error: {{e}}");

                let q = quote! {
                    fn #method_name(#inputs) #return_type {
                        let rc_env = &self.env;

                        #(#transformations)*

                        let method_args: &[jni::objects::JValue] = &[#(#params_into_array,)*]; //vec![s.into()];

                        let result = {
                            let mut env = rc_env.borrow_mut();
                            let r = env.call_method(&self.callback, #str_method_name, #method_types_signature, method_args);

                            if env.exception_check().expect("Failed to check if there is an exception") {
                                env.exception_describe();
                            }

                            r.unwrap_or_else(|e| panic!(#error_msg))
                        };
                    }
                };

                Some(q)
            } else {
                None
            }
        })
        .collect::<Vec<TokenStream2>>();

    let functions_to_serialize = item_trait
        .items
        .iter()
        .filter_map(|item| {
            if let TraitItem::Fn(method) = item {
                let method_name = &method.sig.ident;
                let parameters = get_parameters_from_method(&method.sig.inputs);
                let return_type = get_return_type_from_method(&method.sig.output);

                Some(Function {
                    owner_name: trait_name.clone(),
                    name: method_name.to_string(),
                    parameters,
                    return_type,
                })
            } else {
                None
            }
        })
        .collect::<Vec<Function>>();

    let interface = RsInterface {
        name: trait_name,
        functions: functions_to_serialize,
    };

    let interface_json = serde_json::to_string(&interface).unwrap();

    let header_param = format!("JNI_INTERFACE {interface_json}");
    let header_comments = full_header_comment(header_param.as_str());

    let out = quote! {
        #header_comments
        #item_trait

        struct #trait_implementer_name<'a> {
            env: std::rc::Rc<std::cell::RefCell<jni::JNIEnv<'a>>>,
            callback: std::rc::Rc<jni::objects::JObject<'a>>,
        }
        
        impl<'a> #trait_token for #trait_implementer_name<'a> {
            #(#functions)*
        }
    };

    out.into()
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
        JniType::UInt64 => {
            quote! {
                let #param = #struct_parameter as jni::sys::jlong; // TODO This should be unsigned, perhaps use an object?
            }
        }
        JniType::Float32 => {
            quote! {
                let #param = #struct_parameter as jni::sys::jfloat; // TODO This should be unsigned, perhaps use an object?
            }
        }
        JniType::Float64 => {
            quote! {
                let #param = #struct_parameter as jni::sys::jdouble; // TODO This should be unsigned, perhaps use an object?
            }
        }
        JniType::String => {
            quote! {
                let #param: jni::objects::JString = {
                    let mut env = rc_env.borrow_mut();
                    #struct_parameter.into_env(&mut env)
                };
                let #param: jni::objects::JObject = #param.into();
                let #param: jni::objects::JValue = jni::objects::JValue::Object(&#param);
            }
        }
        JniType::Boolean => {
            quote! {
                let #param = #struct_parameter as jni::sys::jboolean;
            }
        }
        JniType::ByteArray => {
            quote! {
                let #param: jni::objects::JByteArray = {
                    let mut env = rc_env.borrow_mut();
                    #struct_parameter.into_env(&mut env)
                };
                let #param: jni::objects::JValue = jni::objects::JValue::Object(&#param);
            }
        }
        JniType::Receiver(_) | JniType::CustomType(_) => {
            quote! {
                let #param: jni::objects::JObject = {
                    let mut env = rc_env.borrow_mut();
                    #struct_parameter.into_env(&mut env)
                };
                let #param: jni::objects::JValue = jni::objects::JValue::Object(&#param);
            }
        }
        JniType::Interface(_) => todo!(),
        JniType::Void => todo!(),
        JniType::Option(ty) => {
            
            todo!()
            // quote! {
            //     env.is_same_object(&callback, JObject::null());
            // }
        },
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
                    let ty = quote! {#ty}.to_string();
                    let ty_name = ty
                        .replace('&', "")
                        .replace("mut", "")
                        .trim()
                        .to_string();
                    
                    let is_borrow = ty.contains('&');
                    let is_mutable = ty.contains("mut");
                    
                    Parameter::Typed {
                        name: quote! {#pat}.to_string(),
                        ty: ty_name.into(),
                        is_borrow,
                        is_mutable,
                    }
                }
            }
        })
        .collect()
}
