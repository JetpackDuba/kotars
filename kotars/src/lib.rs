mod types_transformations;

extern crate proc_macro;

use proc_macro::TokenStream;

use quote::{quote, ToTokens};
use syn::{FnArg, ImplItem, ItemImpl, ItemStruct, ItemTrait, LitStr, parse_macro_input, ReturnType, TraitItem, Visibility};
use syn::__private::{str, TokenStream2};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use kotars_common::{Field, Function, JniType, Parameter, RsStruct, string_to_camel_case};
use crate::types_transformations::{transform_jni_type_to_rust, transform_rust_to_jni_type};

const PKG_NAME: &str = "<PKG_NAME>";

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

    let functions = input_impl.items.iter().filter_map(|item| {
        if let ImplItem::Fn(method) = item {
            let method_name = &method.sig.ident;
            let parameters = get_parameters_from_method(&method.sig.inputs);
            let return_type = get_return_type_from_method(&method.sig.output);
            
            Some(
                Function {
                    owner_name: struct_name.clone(),
                    name: method_name.to_string(),
                    parameters,
                    return_type,
                }
            )
        } else {
            None
        }
    }).collect::<Vec<Function>>();

    let new_functions = generate_rust_functions(&struct_name, &functions);

    let output = quote! {
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

            Field {
                is_public: matches!(field.vis, Visibility::Public {  .. }),
                name,
                ty: jni_ty,
            }
        })
        .collect::<Vec<Field>>();

    let constructor_types_signature = fields.iter()
        .map(|field| {
            jni_type_to_jni_method_signature_type(&field.ty)
        })
        .collect::<Vec<String>>()
        .join("");

    let (transformations, params_into_array): (Vec<TokenStream2>, Vec<TokenStream2>) = fields.iter()
        .enumerate()
        .map(|(index, field)| {
            let name = field.name.as_ref();
            let ty = &field.ty;
            let param = match name {
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

            let transformation = rust_property_to_jni_type(ty, &param, &struct_parameter);
            let param_into_array = generate_field_mapping_into_array(ty, &param);

            (transformation, param_into_array)
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
        /// JNI_DATA_CLASS {struct_json}
        /// JNI_BINDING_END
        "#).trim_start().to_string();

    let header_comments: TokenStream2 = syn::parse_str(&header_comments).unwrap();

    let out = quote! {
        #item_struct

        #header_comments
        impl <'local> crate::IntoEnv<'local, jni::objects::JObject<'local>> for #struct_token {
            fn into_env(self, env: &mut jni::JNIEnv<'local>) -> jni::objects::JObject<'local> {
                let package_name_for_signature = crate::JNI_PACKAGE_NAME.replace(".", "/");

                let class_path = if package_name_for_signature.is_empty() {
                    format!("{}", #struct_name)
                } else {
                    format!("{}/{}", package_name_for_signature, #struct_name)
                };

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

fn generate_field_mapping_into_array(ty: &JniType, param: &TokenStream2) -> TokenStream2 {
    match ty {
        JniType::Int32 => {
            quote! { #param.into() }
        }
        JniType::Int64 => {
            quote! { #param.into() }
        }
        JniType::String => {
            quote! { #param }
        }
        JniType::Boolean => {
            quote! { #param.into() }
        }
        JniType::CustomType(_) => {
            quote! { #param }
        }
        JniType::Receiver(_) => panic!("Structs can not have self as type")
    }
}

#[proc_macro_attribute]
pub fn jni_class(_attr: TokenStream, input: TokenStream) -> TokenStream {
    //todo: if all parameters of the struct are public, we should allow instantiating the method from Kotlin
    let item_struct = parse_macro_input!(input as ItemStruct);
    let struct_name = item_struct.ident.to_string();
    let struct_token = &item_struct.ident;

    let constructor_types_signature = jni_type_to_jni_method_signature_type(&JniType::Int64);
    let constructor_signature = format!("({constructor_types_signature})V");

    let rs_struct = RsStruct {
        name: item_struct.ident.to_string(),
        fields: vec![], //TODO THIS
    };

    let struct_json = serde_json::to_string(&rs_struct).unwrap();
    let header_comments = format!(r#"
        /// JNI_BINDING_START
        /// Auto generated header
        /// JNI_CLASS {struct_json}
        /// JNI_BINDING_END
        "#).trim_start().to_string();

    let header_comments: TokenStream2 = syn::parse_str(&header_comments).unwrap();

    let drop_func_header = format!("Java_{struct_token}Obj_destroy");
    let drop_func_header: TokenStream2 = syn::parse_str(&drop_func_header).unwrap();

    let out = quote! {
        #item_struct

        #header_comments
        impl <'local> crate::IntoEnv<'local, jni::objects::JObject<'local>> for #struct_token {
            fn into_env(self, env: &mut jni::JNIEnv<'local>) -> jni::objects::JObject<'local> {
                let package_name_for_signature = crate::JNI_PACKAGE_NAME.replace(".", "/");

                let class_path = if package_name_for_signature.is_empty() {
                    format!("{}", #struct_name)
                } else {
                    format!("{}/{}", package_name_for_signature, #struct_name)
                };

                let pointer = Box::into_raw(Box::new(self)) as jni::sys::jlong;
                let constructor_signature = #constructor_signature.replace(#PKG_NAME, package_name_for_signature.as_str());

                let class = env.find_class(class_path).expect("Find class failed");

                let constructor_args: &[jni::objects::JValue] = &[pointer.into()]; //vec![s.into()];
                let obj = env.new_object(class, constructor_signature.as_str(), constructor_args).expect("New object failed");
                obj
            }
        }

        #[no_mangle]
        pub unsafe extern "system" fn #drop_func_header(
            _env: jni::JNIEnv,
            _class: jni::objects::JClass,
            jni_pointer: jni::sys::jlong,
        ) {
            drop(Box::from_raw(jni_pointer as *mut #struct_token))
        }

    };

    out.into()
}

#[proc_macro_attribute]
pub fn jni_interface(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let item_trait = parse_macro_input!(input as ItemTrait);
    let trait_name = item_trait.ident.to_string();
    let trait_token = &item_trait.ident;

    let constructor_types_signature = jni_type_to_jni_method_signature_type(&JniType::Int64);
    let constructor_signature = format!("({constructor_types_signature})V");

    let rs_struct = RsStruct {
        name: item_trait.ident.to_string(),
        fields: vec![], //TODO THIS
    };

    let functions = item_trait.items.iter().filter_map(|item| {
        if let TraitItem::Fn(method) = item {
            let method_name = &method.sig.ident;

            println!("Loading parameters");
            let parameters = get_parameters_from_method(&method.sig.inputs);
            println!("Done with parameters, loading return type");
            let return_type = get_return_type_from_method(&method.sig.output);
            println!("Done with return type");
            Some(
                Function {
                    owner_name: trait_name.clone(),
                    name: method_name.to_string(),
                    parameters,
                    return_type,
                }
            )
        } else {
            None
        }
    }).collect::<Vec<Function>>();

    let functions_impl = item_trait.items.iter().filter_map(|item| {
        if let TraitItem::Fn(method) = item {
            let method_name = &method.sig.ident;

            println!("Loading parameters");
            let parameters = get_parameters_from_method(&method.sig.inputs);
            println!("Done with parameters, loading return type");
            let return_type = get_return_type_from_method(&method.sig.output);
            println!("Done with return type");
            Some(
                Function {
                    owner_name: trait_name.clone(),
                    name: method_name.to_string(),
                    parameters,
                    return_type,
                }
            )
        } else {
            None
        }
    }).collect::<Vec<Function>>();

    let trait_implementer_name = format!("{trait_name}JniBridge");
    let trait_implementer_name = syn::parse_str::<TokenStream2>(&trait_implementer_name).unwrap();
    
    let out = quote! {
        #item_trait

        struct #trait_implementer_name<'a> {
            env: &'a mut jni::JNIEnv<'a>,
            obj: &'a jni::objects::JObject<'a>,
        }

        impl #trait_token for #trait_implementer_name {

        }
    };

    out.into()
}

fn rust_property_to_jni_type(ty: &JniType, param: &TokenStream2, struct_parameter: &TokenStream2) -> TokenStream2 {
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
    struct_name: &str,
    functions: &[Function],
) -> Vec<TokenStream2> {
    functions.iter().map(|func| {
        let fn_name = &func.name;
        let fn_name_for_jni = string_to_camel_case(fn_name);

        let obj_suffix = "Obj";

        // Generate the new function body with parameter inspection
        let method_name = format!("Java_{struct_name}{obj_suffix}_{fn_name_for_jni}");

        let mut jni_function_params: Vec<TokenStream2> = vec![
            quote! { mut env: jni::JNIEnv<'local> },
            quote! { _class: jni::objects::JClass<'local> },
        ];

        let mut transformations: Vec<TokenStream2> = Vec::new();

        for param in &func.parameters {
            match param {
                Parameter::Typed { name, ty } => {
                    let name = name.to_string();
                    let transformation = transform_jni_type_to_rust(ty, &name);
                    let rust_jni_ty = jni_type_to_jni_type(ty);

                    let name = syn::parse_str::<TokenStream2>(&name).unwrap();
                    jni_function_params.push(quote! { #name: #rust_jni_ty });
                    transformations.push(transformation);
                }
                Parameter::Receiver { .. } => {
                    let name = "jobject".to_string();
                    let ty = JniType::Receiver(struct_name.to_string());

                    let jni_ty = jni_type_to_jni_type(&ty);
                    let name_token = syn::parse_str::<TokenStream2>(&name).unwrap();
                    jni_function_params.push(quote! { #name_token: #jni_ty });

                    let transformation = transform_jni_type_to_rust(&ty, &name);
                    transformations.push(transformation);
                }
            }
        }

        let fn_owner = syn::parse_str::<TokenStream2>(struct_name).unwrap();
        let fn_to_call = syn::parse_str::<TokenStream2>(fn_name).unwrap();

        let rust_fn_call_params: Vec<TokenStream2> = func.parameters
            .iter()
            .map(|param| {
                match param {
                    Parameter::Typed { name, ty: _ty } => {
                        syn::parse_str::<TokenStream2>(name).unwrap()
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
                let ret_type = jni_type_to_jni_type(ty);
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

        println!("Serialized fn is {fn_serialized}");

        let header_comments = format!(r#"
        /// JNI_BINDING_START
        /// JNI_FN_DATA {fn_serialized}
        /// JNI_BINDING_END
        /// Auto generated header. This will be used by cargo-kotars to generate the Kotlin code that binds to the Rust code.
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

fn jni_type_to_jni_type(jni_type: &JniType) -> TokenStream2 {
    match jni_type {
        JniType::Int32 => quote! { jni::sys::jint },
        JniType::Int64 => quote! { jni::sys::jlong },
        JniType::String => quote! { jni::objects::JString<'local> },
        JniType::Boolean => quote! { jni::sys::jboolean },
        JniType::CustomType(_) => quote! { jni::objects::JObject<'local> },
        JniType::Receiver(_) => quote! { jni::sys::jlong },
    }
}

fn jni_type_to_jni_method_signature_type(jni_type: &JniType) -> String {
    match jni_type {
        JniType::Int32 => "I".to_string(),
        JniType::Int64 | JniType::Receiver(_) => "J".to_string(),
        JniType::String => "Ljava/lang/String;".to_string(),
        JniType::Boolean => "Z".to_string(),
        JniType::CustomType(name) => {
            format!("L{PKG_NAME}/{name};")
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

