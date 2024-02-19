extern crate proc_macro;

use proc_macro::TokenStream;

use quote::{quote, ToTokens};
use serde::{Deserialize, Serialize};
use syn::{FnArg, ImplItem, ItemFn, ItemImpl, ItemStruct, LitStr, parse_macro_input, ReturnType};
use syn::__private::{str, TokenStream2};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use kotars_common::{Function, JniType, Parameter, RsStruct};

const PKG_NAME: &str = "<PKG_NAME>";

#[proc_macro]
pub fn jni_init(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);

    let package_name = quote! { #input };
    println!("Package name: {package_name}");

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
pub fn jni(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let file_name = "HelloWorld".to_string();

    // Parse the function item
    let input = parse_macro_input!(item as ItemFn);
    let fn_name = input.sig.ident.to_string();
    // Extract parameter names and types
    let parameters: Vec<Parameter> = get_parameters_from_method(&input.sig.inputs);
    let return_type = get_return_type_from_method(&input.sig.output);

    let function = Function {
        name: fn_name.clone(),
        parameters: parameters.clone(),
        return_type: return_type.clone(),
    };

    let functions = vec![function];
    // generate_kotlin_source(&file_name, &functions);
    // generate_jni_header(&file_name, &functions);

    let new_functions = generate_rust_functions(&file_name, &functions);

    let new_func = quote! {
        #input

        #(#new_functions)*
    };

    new_func.into()
}


#[proc_macro_attribute]
pub fn jni_struct_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_impl = parse_macro_input!(item as ItemImpl);

    // Extract the struct name
    let struct_name = input_impl.self_ty.as_ref().to_token_stream().to_string();
    println!("Struct name: {}", struct_name);

    let functions = input_impl.items.iter().filter_map(|item| {
        if let ImplItem::Fn(method) = item {
            let method_name = &method.sig.ident;

            println!("Loading parameters");
            let parameters = get_parameters_from_method(&method.sig.inputs);
            println!("Done with parameters, loading return type");
            let return_type = get_return_type_from_method(&method.sig.output);
            println!("Done with return type");
            Some(
                Function {
                    name: method_name.to_string(),
                    parameters,
                    return_type,
                }
            )
        } else {
            None
        }
    }).collect::<Vec<Function>>();

    // generate_kotlin_source(&struct_name, &functions);
    let new_functions = generate_rust_functions(&struct_name, &functions);

    let output = quote! {
        // You can use struct_name and functions wherever necessary in your generated code
        #input_impl
        #(#new_functions)*
    };

    output.into()
}

#[proc_macro_attribute]
pub fn jni_data_class(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let item_struct = parse_macro_input!(input as ItemStruct);
    let struct_name = item_struct.ident.to_string();
    let struct_token = &item_struct.ident;
    println!("item_struct is {struct_name}");

    let fields = item_struct.fields
        .iter()
        .map(|field| {
            let name = field.ident.as_ref().map(|id| quote! { #id }.to_string());
            let original_ty = &field.ty;
            let ty = quote! { #original_ty }.to_string();
            let jni_ty: JniType = ty.into();

            (name, jni_ty)
        })
        .collect::<Vec<(Option<String>, JniType)>>();

    let constructor_types_signature = fields.iter()
        .map(|(_, ty)| {
            jni_type_to_jni_method_signature_type(ty)
        })
        .collect::<Vec<String>>()
        .join("");

    let (transformations, params_into_array): (Vec<TokenStream2>, Vec<TokenStream2>) = fields.iter()
        .enumerate()
        .map(|(index, (name, ty))| {
            let param = match name.as_ref() {
                None => {
                    let param_name = format!("param{index}");
                    syn::parse_str::<TokenStream2>(&param_name).unwrap()
                }
                Some(param_name) => syn::parse_str::<TokenStream2>(param_name).unwrap()
            };
            let struct_parameter = match name.as_ref() {
                None => quote! { self.#index },
                Some(param_name) => {
                    let param_name: TokenStream2 = syn::parse_str(param_name).unwrap();
                    quote! { self.#param_name }
                }
            };


            match ty {
                JniType::Int32 => {
                    let transformation = quote! {
                        let #param = #struct_parameter as jni::sys::jint;
                    };

                    let param_into_array = quote! { #param.into() };
                    (transformation, param_into_array)
                }
                JniType::String => {
                    let transformation = quote! {
                        let #param: jni::objects::JString = #struct_parameter.into_env(env);
                        let #param: jni::objects::JObject = #param.into();
                        let #param: jni::objects::JValue = jni::objects::JValue::Object(&#param);
                    };

                    let param_into_array = quote! { #param };
                    (transformation, param_into_array)
                }
                JniType::Boolean => {
                    let transformation = quote! {
                        let #param = #struct_parameter as jni::sys::jboolean;
                    };

                    let param_into_array = quote! { #param.into() };
                    (transformation, param_into_array)
                }
                JniType::CustomType(_) => {
                    let transformation = quote! {
                        let #param: jni::objects::JObject = #struct_parameter.into_env(env);
                        let #param: jni::objects::JValue = jni::objects::JValue::Object(&#param);
                    };

                    let param_into_array = quote! { #param };
                    (transformation, param_into_array)
                }
            }
        })
        .collect::<Vec<(TokenStream2, TokenStream2)>>()
        .into_iter()
        .unzip();

    let constructor_signature = format!("({constructor_types_signature})V");

    let rs_struct = RsStruct {
        name: item_struct.ident.to_string(),
        fields,
    };

    let struct_json = serde_json::to_string(&rs_struct).unwrap();
    let header_comments = format!(r#"
        /// JNI_BINDING_START
        /// Auto generated header
        /// Struct: {struct_name}
        /// DataClass: {struct_json}
        /// JNI_BINDING_END
        "#).trim_start().to_string();

    let header_comments: TokenStream2 = syn::parse_str(&header_comments).unwrap();

    let out = quote! {
        #item_struct

        #header_comments
        impl <'local> crate::IntoEnv<'local, jni::objects::JObject<'local>> for #struct_token {
            fn into_env(self, env: &mut jni::JNIEnv<'local>) -> jni::objects::JObject<'local> {
                let package_name_for_signature = crate::JNI_PACKAGE_NAME.replace(".", "/");
                let class_path = format!("{}/{}", package_name_for_signature, #struct_name);
                println!("Class path is: {class_path}");

                let constructor_signature = #constructor_signature.replace(#PKG_NAME, package_name_for_signature.as_str());

                let class = env.find_class(class_path).unwrap();
                #(#transformations)*

                let constructor_args: &[jni::objects::JValue] = &[#(#params_into_array,)*]; //vec![s.into()];
                let obj = env.new_object(class, constructor_signature.as_str(), constructor_args).unwrap();
                obj
            }
        }
    };

    out.into()
}

fn get_return_type_from_method(return_type: &ReturnType) -> Option<JniType> {
    match return_type {
        ReturnType::Default => { None }
        ReturnType::Type(_token, typ) => {
            let rt: JniType = quote::quote!(#typ)
                .to_string()
                .into();

            Some(rt)
        }
    }
}

fn generate_rust_functions(
    file_name: &str,
    functions: &[Function],
) -> Vec<TokenStream2> {
    functions.iter().map(|func| {
        let fn_name = &func.name;
        let fn_name_for_jni = string_to_camel_case(fn_name);

        // Generate the new function body with parameter inspection
        let method_name = format!("Java_{file_name}_{fn_name_for_jni}");

        let mut jni_function_params: Vec<TokenStream2> = vec![
            quote! { mut env: jni::JNIEnv<'local> },
            quote! { _class: jni::objects::JClass<'local> },
        ];

        let mut transformations: Vec<TokenStream2> = Vec::new();
        // let method_call_prefix = String::new(); // TODO we should call the prefix if it's a struct "static" method

        let rust_fn_call_params: Vec<TokenStream2> = func.parameters
            .iter()
            .map(|param| {
                let name = &param.name;
                syn::parse_str::<TokenStream2>(name).unwrap()
            })
            .collect();


        for param in &func.parameters {
            let name = param.name.to_string();
            let transformation = transform_jni_type_to_rust(&param.ty, &name);
            let rust_jni_ty = jni_type_to_rust_jni_type(&param.ty);

            let name = syn::parse_str::<TokenStream2>(&name).unwrap();
            jni_function_params.push(quote! { #name: #rust_jni_ty });
            transformations.push(transformation);
        }

        let fn_owner = syn::parse_str::<TokenStream2>(file_name).unwrap();
        let fn_to_call = syn::parse_str::<TokenStream2>(fn_name).unwrap();
        let rust_fn_call = quote! { let result = <#fn_owner>::#fn_to_call(#(#rust_fn_call_params,)*); };

        let return_signature = match &func.return_type {
            None => { quote! {} }
            Some(ty) => {
                let ret_type = jni_type_to_rust_jni_type(ty);
                quote! { -> #ret_type }
            }
        };

        let result_variable = quote! { result };

        let (transform_return, return_statement) = if let Some(ty) = &func.return_type {
            let transform = transform_rust_to_jni_type(ty, &result_variable.to_string());
            let return_statement = quote! { return #result_variable; };
            (transform, return_statement)
        } else {
            (quote!(), quote!())
        };

        let method_name_token_stream = syn::parse_str::<TokenStream2>(method_name.as_str()).unwrap();
        let fn_serialized = serde_json::to_string(func).unwrap_or_else(|_| panic!("Serialization of function {fn_name} failed"));

        let header_comments = format!(r#"
        /// JNI_BINDING_START
        /// Auto generated header
        /// StructImpl: {file_name}
        /// Fn: {fn_serialized}
        /// JNI_BINDING_END
        "#).trim_start().to_string();

        let header_comments: TokenStream2 = syn::parse_str(&header_comments).unwrap();

        quote! {
            #header_comments
            #[no_mangle]
            pub extern "system" fn #method_name_token_stream<'local>(
                #(#jni_function_params),*
            ) #return_signature {
                #(#transformations)*
                #rust_fn_call
                #transform_return
                #return_statement
            }
        }
    }).collect()
}

// fn generate_kotlin_source(file_name: &str, functions: &Vec<Function>) {
//     let mut kotlin_out = format!("object {file_name} ");
//     kotlin_out.push('{');
//     kotlin_out.push('\n');
//
//     for func in functions {
//         kotlin_out.push_str(&format!("external fun {} (\n", func.name));
//
//         let kotlin_params_str = generate_kotlin_parameters(&func.parameters);
//         kotlin_out.push_str(&kotlin_params_str);
//         kotlin_out.push_str("\n)");
//
//         if let Some(rt) = &func.return_type {
//             let kotlin_type = jni_type_to_kotlin_string(rt);
//             kotlin_out.push_str(&format!(": {kotlin_type}"));
//         }
//
//         kotlin_out.push('\n');
//     }
//
//     kotlin_out.push_str("\n}");
//     let mut file = File::create(format!("{OUT_PATH}/{file_name}.kt")).unwrap();
//     file.write_all(kotlin_out.as_bytes()).unwrap();
// }

// fn generate_kotlin_parameters(source_parameters: &[Parameter]) -> String {
//     source_parameters
//         .iter()
//         .map(|parameter: &Parameter| {
//             let name = &parameter.name;
//             let ty = jni_type_to_kotlin_string(&parameter.ty);
//
//             format!("{name}: {ty}")
//         })
//         .collect::<Vec<String>>()
//         .join(",\n")
// }


fn string_to_camel_case(text: &str) -> String {
    text
        .to_string()
        .split(['_', ' '])
        .enumerate()
        .map(|(index, word)| {
            let word = word.to_string();
            if index == 0 || word.is_empty() {
                word
            } else {
                let mut letters = word.chars().collect::<Vec<char>>();

                letters[0] = letters[0]
                    .to_uppercase()
                    .to_string()
                    .chars()
                    .collect::<Vec<char>>()[0];

                letters.iter().collect::<String>()
            }
        })
        .collect::<Vec<String>>()
        .join("")
}

fn jni_type_to_rust_jni_type(jni_type: &JniType) -> TokenStream2 {
    match jni_type {
        JniType::Int32 => quote! { jni::sys::jint },
        JniType::String => quote! { jni::objects::JString<'local> },
        JniType::Boolean => quote! { jni::sys::jboolean },
        JniType::CustomType(_) => quote! { jni::objects::JObject<'local> },
    }
}

// fn jni_type_to_kotlin_string(jni_type: &JniType) -> &str {
//     match jni_type {
//         JniType::Int32 => "Int",
//         JniType::String => "String",
//         JniType::Boolean => "Boolean",
//         JniType::CustomType(name) => name,
//     }
// }

fn jni_type_to_jni_method_signature_type(jni_type: &JniType) -> String {
    match jni_type {
        JniType::Int32 => "I".to_string(),
        JniType::String => "Ljava/lang/String;".to_string(),
        JniType::Boolean => "Z".to_string(),
        JniType::CustomType(name) => {
            format!("L{PKG_NAME}/{name};")
        }
    }
}

fn transform_jni_type_to_rust(jni_type: &JniType, param_name: &str) -> TokenStream2 {
    match jni_type {
        JniType::Int32 => transform_jint_to_i32(param_name),
        JniType::String => transform_jstring_to_string(param_name),
        JniType::Boolean => transform_jbool_to_bool(param_name),
        JniType::CustomType(_) => transform_jobject_to_custom(param_name),
    }
}

fn transform_rust_to_jni_type(jni_type: &JniType, param_name: &str) -> TokenStream2 {
    match jni_type {
        JniType::Int32 => transform_i32_to_jint(param_name),
        JniType::String => transform_string_to_jstring(param_name),
        JniType::Boolean => transform_bool_to_jbool(param_name),
        JniType::CustomType(_) => transform_custom_to_jobject(param_name),
    }
}

fn transform_jint_to_i32(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { i32 })
}

fn transform_i32_to_jint(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { jni::sys::jint })
}

fn transform_bool_to_jbool(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { jni::sys::jboolean })
}

fn transform_custom_to_jobject(param_name: &str) -> TokenStream2 {
    let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! { let #param = #param.into_env(&mut env); }
}

fn transform_jobject_to_custom(param_name: &str) -> TokenStream2 {
    let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! { let #param = #param.into_env(&mut env); }
}

fn transform_jbool_to_bool(param_name: &str) -> TokenStream2 {
    let transform = transform_types(param_name, quote! { u8 });
    let param_name = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! {
        #transform
        let #param_name = #param_name == 1;
    }
}

fn transform_types(param_name: &str, target_type: TokenStream2) -> TokenStream2 {
    let param_name = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! { let #param_name = #param_name as #target_type; }
}

fn transform_jstring_to_string(param_name: &str) -> TokenStream2 {
    let param_name = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! {
        let #param_name: String = env
            .get_string(&#param_name)
            .expect("Couldn't get java string!")
            .into();
    }
}

fn transform_string_to_jstring(param_name: &str) -> TokenStream2 {
    let param_name = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! {
        let #param_name = env
            .new_string(#param_name)
            .expect("Couldn't create java string!");
    }
}


fn get_parameters_from_method(inputs: &Punctuated<FnArg, Comma>) -> Vec<Parameter> {
    inputs
        .iter()
        .map(|param| {
            if let FnArg::Typed(ref pat_type) = param {
                let pat = &pat_type.pat;
                let ty = &pat_type.ty;

                Parameter {
                    name: quote! {#pat}.to_string(),
                    ty: quote! {#ty}.to_string().into(),
                }
            } else {
                panic!("Unsupported function argument type");
            }
        })
        .collect()
}

