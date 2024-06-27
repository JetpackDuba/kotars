use quote::quote;
use syn::__private::TokenStream2;
use kotars_common::{Function, JniType, Parameter, string_to_camel_case};
use crate::types_transformations::{transform_jni_type_to_rust, transform_rust_to_jni_type};
use crate::full_header_comment;

pub fn generate_rust_jni_binding_functions(
    struct_name: &str,
    functions: &[Function],
) -> Vec<TokenStream2> {
    functions.iter().map(|func| {
        generate_rust_jni_binding_function(struct_name, func)
    }).collect()
}

fn generate_rust_jni_binding_function(struct_name: &str, func: &Function) -> TokenStream2 {
    let fn_name = &func.name;
    let fn_name_for_jni = string_to_camel_case(fn_name);

    let obj_suffix = "Obj";

    // Name of the JNI method containing
    let method_name = format!("Java_{struct_name}{obj_suffix}_{fn_name_for_jni}");

    let mut jni_function_parameters: Vec<TokenStream2> = vec![
        quote! { mut env: jni::JNIEnv<'local> },
        quote! { _class: jni::objects::JClass<'local> },
    ];

    let mut jni_to_rust_types_transformations: Vec<TokenStream2> = Vec::new();

    for param in &func.parameters {
        match param {
            Parameter::Typed {
                name,
                ty,
                is_borrow,
                is_mutable
            } => {
                let name = name.to_string();
                let rust_jni_ty = jni_type_to_jni_type(ty, false);
                let transformation = transform_jni_type_to_rust(ty, &name, false);

                let name = syn::parse_str::<TokenStream2>(&name).unwrap();
                jni_function_parameters.push(quote! { #name: #rust_jni_ty });
                jni_to_rust_types_transformations.push(transformation);
            }
            Parameter::Receiver { .. } => {
                let name = "jobject".to_string();
                let ty = JniType::Receiver(struct_name.to_string());

                let jni_ty = jni_type_to_jni_type(&ty, false);
                let name_token = syn::parse_str::<TokenStream2>(&name).unwrap();
                jni_function_parameters.push(quote! { #name_token: #jni_ty });

                let transformation = transform_jni_type_to_rust(&ty, &name, false);
                jni_to_rust_types_transformations.push(transformation);
            }
        }
    }

    let fn_owner = syn::parse_str::<TokenStream2>(struct_name).unwrap();
    let fn_to_call = syn::parse_str::<TokenStream2>(fn_name).unwrap();

    let rust_fn_call_params: Vec<TokenStream2> = func.parameters
        .iter()
        .map(|param| {
            match param {
                Parameter::Typed {
                    name,
                    ty,
                    is_borrow,
                    is_mutable
                } => {
                    let name = rust_fn_call_from_jni_type(ty, name);
                    syn::parse_str::<TokenStream2>(&name).unwrap()
                }
                Parameter::Receiver { is_mutable } => {
                    let mutability_prefix = if *is_mutable {
                        String::from("mut")
                    } else {
                        String::new()
                    };

                    let jobject_param = format!("&{mutability_prefix} jobject");
                    syn::parse_str::<TokenStream2>(&jobject_param).unwrap()
                }
            }
        })
        .collect();

    let rust_fn_call = quote! { let result = <#fn_owner>::#fn_to_call(#(#rust_fn_call_params,)*); };

    let return_signature = match &func.return_type {
        None => { quote! {} }
        Some(ty) => {
            let ret_type = jni_type_to_jni_type(ty, false);
            quote! { -> #ret_type }
        }
    };

    let result_variable = quote! { result };

    let (transform_return, return_statement) = if let Some(ty) = &func.return_type {
        let transform = transform_rust_to_jni_type(ty, &result_variable.to_string(), false);
        let return_statement = quote! { return #result_variable; };
        (transform, return_statement)
    } else {
        (quote!(), quote!())
    };

    let method_name_token_stream = syn::parse_str::<TokenStream2>(method_name.as_str()).unwrap();
    let fn_serialized = serde_json::to_string(func).unwrap_or_else(|_| panic!("Serialization of function {fn_name} failed"));

    let header_param = format!("JNI_FN_DATA {fn_serialized}");
    let header_comments = full_header_comment(header_param.as_str());

    let contains_trait_param = func.parameters
        .iter()
        .any(|p| matches!(p, Parameter::Typed { ty: JniType::Interface(_), .. }));

    println!("!Func {} has trait parameter = {}", func.name, contains_trait_param);

    quote! {
            #header_comments
            #[no_mangle]
            pub extern "system" fn #method_name_token_stream<'local>(
                #(#jni_function_parameters),*
            ) #return_signature {
                let rc_env = std::rc::Rc::new(std::cell::RefCell::new(env));

                #(#jni_to_rust_types_transformations)*

                #rust_fn_call
                #transform_return
                #return_statement
            }
        }
}

fn rust_fn_call_from_jni_type(jni_type: &JniType, name: &String) -> String {
    match jni_type {
        JniType::Int32 | JniType::Int64 | JniType::UInt64 | JniType::String | JniType::Boolean => { name.clone() }
        JniType::Receiver(_) => { todo!() }
        JniType::ByteArray => { format!("& {name}") }
        JniType::CustomType(_) => { format!("&mut {name}") }
        JniType::Void => { todo!() }
        JniType::Option(ty) => { rust_fn_call_from_jni_type(ty, name) }
        JniType::Interface(_) => { format!("&mut {name}") }
    }
}

fn jni_type_to_jni_type(jni_type: &JniType, is_optional: bool) -> TokenStream2 {
    if is_optional {
        quote! { jni::objects::JObject<'local> }
    } else {
        match jni_type {
            JniType::Int32 => quote! { jni::sys::jint },
            JniType::Int64 => quote! { jni::sys::jlong },
            JniType::UInt64 => quote! { jni::sys::jlong }, // TODO This should be unsigned, perhaps use an object?
            JniType::String => quote! { jni::objects::JString<'local> },
            JniType::Boolean => quote! { jni::sys::jboolean },
            JniType::ByteArray => quote! { jni::objects::JByteArray },
            JniType::Interface(_) | JniType::CustomType(_) => quote! { jni::objects::JObject<'local> },
            JniType::Receiver(_) => quote! { jni::sys::jlong },
            JniType::Void => todo!(),
            JniType::Option(ty) => jni_type_to_jni_type(ty, true),
        }
    }
}