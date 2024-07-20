use quote::quote;
use syn::__private::TokenStream2;
use kotars_common::JniType;

pub fn transform_jni_type_to_rust(
    jni_type: &JniType,
    param_name: &str,
    is_optional: bool,
) -> TokenStream2 {
    match jni_type {
        JniType::Int32 => transform_jint_to_i32(param_name, is_optional),
        JniType::Int64 => transform_jlong_to_i64(param_name),
        JniType::UInt64 => transform_jlong_to_u64(param_name), // TODO This should be unsigned, perhaps use an object?
        JniType::Float32 => transform_jfloat_to_f32(param_name),
        JniType::Float64 => transform_jdouble_to_f64(param_name),
        JniType::String => transform_jstring_to_string(param_name, is_optional),
        JniType::Boolean => transform_jbool_to_bool(param_name),
        JniType::ByteArray => {
            let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
            quote! {
                let #param: Vec<u8> = {
                    let mut env = rc_env.borrow_mut();
                    #param.into_env(&mut env)
                };
            }
        }
        JniType::CustomType(ty) => transform_jobject_to_custom(param_name, ty),
        JniType::Receiver(ty) => transform_jlong_to_receiver(param_name, ty),
        JniType::Void => panic!("Void can't be transformed to a Rust type"),

        JniType::Option(ty) => {
            let transform = transform_jni_type_to_rust(ty, param_name, true);
            let param = syn::parse_str::<TokenStream2>(param_name).unwrap();

            let q = quote! {
                let is_null = {
                    let mut env = rc_env.borrow_mut();
                    env.is_same_object(&#param, jni::objects::JObject::null()).expect("Could not check if object is null")
                };
                
                let #param = if is_null {
                    Option::None
                } else {
                    #transform

                    // Test
                    Some(#param)
                };
            };

            let qs = q.to_string();

            println!("QS3 IS {qs}");

            q
        }
        JniType::Interface(name) => {
            let struct_name = format!("{name}JniBridge");
            let struct_name: TokenStream2 = syn::parse_str(&struct_name).unwrap();
            let param: TokenStream2 = syn::parse_str(param_name).unwrap();
            let param_rc_name = format!("rc_{param_name}");
            let param_rc: TokenStream2 = syn::parse_str(&param_rc_name).unwrap();

            quote! {
                let #param_rc = std::rc::Rc::new(#param);

                let mut #param = #struct_name {
                    env: std::rc::Rc::clone(&rc_env),
                    callback: std::rc::Rc::clone(& #param_rc),
                };
            }
        }
    }
}

fn transform_jlong_to_receiver(param_name: &str, ty: &str) -> TokenStream2 {
    let param: TokenStream2 = syn::parse_str(param_name).unwrap();
    let ty: TokenStream2 = syn::parse_str(ty).unwrap();

    quote! {
        let mut #param = unsafe { &mut *(#param as *mut #ty) };
    }
}

pub fn transform_rust_to_jni_type(jni_type: &JniType, param_name: &str, is_optional: bool) -> TokenStream2 {
    match jni_type {
        JniType::Int32 => transform_i32_to_jint(param_name, is_optional),
        JniType::Int64 => transform_i64_to_jlong(param_name),
        JniType::UInt64 => transform_u64_to_jlong(param_name), // TODO This should be unsigned, perhaps use an object?
        JniType::Float32 => transform_f32_to_jfloat(param_name),
        JniType::Float64 => transform_f64_to_jdouble(param_name),
        JniType::String => transform_string_to_jstring(param_name),
        JniType::Boolean => transform_bool_to_jbool(param_name),
        JniType::ByteArray => {
            let param = syn::parse_str::<TokenStream2>(param_name).unwrap();

            quote! {
                let #param: jni::objects::JByteArray = {
                    let mut env = rc_env.borrow_mut();
                    #param.into_env(&mut env)
                };
            }
        }
        JniType::CustomType(_) => transform_custom_to_jobject(param_name),
        JniType::Receiver(_) => todo!(),
        JniType::Option(ty) => transform_rust_to_jni_type(ty, param_name, true),
        JniType::Interface(_) => panic!("Transformation from Rust traits to interfaces is not supported"),
        JniType::Void => panic!("Void type can't be transformed"),
    }
}

fn transform_jint_to_i32(param_name: &str, is_optional: bool) -> TokenStream2 {
    if is_optional {
        let param = syn::parse_str::<TokenStream2>(param_name).unwrap();

        let q = quote! {
            let #param = {
                let mut env = rc_env.borrow_mut();
                let value = env.get_field(&#param, "value", "I")
                    .expect("Could not find field pointer")
                    .i()
                    .expect("Could not transform \"value\" to jint");

                value
            };
        };

        let qs = q.to_string();

        println!("QS2 IS {qs}");

        q
    } else {
        transform_types(param_name, quote! { i32 })
    }
}

fn transform_jlong_to_i64(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { i64 })
}

fn transform_jfloat_to_f32(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { f64 })
}

fn transform_jdouble_to_f64(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { f64 })
}

fn transform_jlong_to_u64(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { u64 })
}

fn transform_i32_to_jint(param_name: &str, is_optional: bool) -> TokenStream2 {
    if is_optional {
        let param = syn::parse_str::<TokenStream2>(param_name).unwrap();

        quote! {
            let #param = {
                match #param {
                    None => {
                        jni::objects::JObject::null()
                    }
                    Some(i) => {
                        let mut env = rc_env.borrow_mut();
                        let values: &[jni::objects::JValue] = &[i.into()];
                        let jv = env
                            .call_static_method("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;", values)
                            .expect("Unable to load ValueOf from java.lang.Integer");

                        jv.l().expect("Could not get Integer type from valueOf result")
                    }
                }
            };
        }
    } else {
        transform_types(param_name, quote! { jni::sys::jint })
    }
}


fn transform_i64_to_jlong(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { jni::sys::jlong })
}

fn transform_u64_to_jlong(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { jni::sys::jlong })
}

fn transform_f32_to_jfloat(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { jni::sys::jfloat })
}

fn transform_f64_to_jdouble(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { jni::sys::jdouble })
}

fn transform_bool_to_jbool(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { jni::sys::jboolean })
}

fn transform_custom_to_jobject(param_name: &str) -> TokenStream2 {
    let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! {
        
        let mut #param = {
            let mut env = rc_env.borrow_mut();
            #param.into_env(&mut env)
        }; 
    }
}

fn transform_jobject_to_custom(param_name: &str, ty: &str) -> TokenStream2 {
    let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
    println!("Ty is after prefix removal: {ty}");
    let ty = syn::parse_str::<TokenStream2>(&ty).unwrap();
    quote! {
        let mut #param = {
            let mut env = rc_env.borrow_mut();
            let field = env.get_field(&#param, "pointer", "J")
                .expect("Could not find field pointer")
                .j()
                .expect("Could not transform pointer to jlong");
            unsafe { &mut *(field as *mut #ty) }
        }; 
    }
}

fn transform_jbool_to_bool(param_name: &str) -> TokenStream2 {
    let transform = transform_types(param_name, quote! { u8 });
    let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! {
        #transform
        let #param = #param == 1;
    }
}

fn transform_types(param_name: &str, target_type: TokenStream2) -> TokenStream2 {
    let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! { let #param = #param as #target_type; }
}

fn transform_jstring_to_string(param_name: &str, is_optional: bool) -> TokenStream2 {
    let param_to_get_string = if is_optional {
        format!("{param_name}.into()")
    } else {
        param_name.to_string()
    };

    let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
    let param_to_get_string = syn::parse_str::<TokenStream2>(&param_to_get_string).unwrap();

    quote! {        
        let #param: String = {
            let mut env = rc_env.borrow_mut();
            
            env
            .get_string(&#param_to_get_string)
            .expect("Couldn't get java string!")
            .into()
        };
    }
}

fn transform_string_to_jstring(param_name: &str) -> TokenStream2 {
    let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! {
        let #param = env
            .new_string(#param)
            .expect("Couldn't create java string!");
    }
}