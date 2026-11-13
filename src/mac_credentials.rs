//! Immutable generic-password items. Secrets never enter argv, environment or logs.
#[cfg(not(test))]
mod native {
    use core_foundation::{
        base::{CFType, CFTypeRef, TCFType},
        boolean::CFBoolean,
        data::CFData,
        dictionary::CFDictionary,
        string::CFString,
    };
    use std::ptr;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        static kSecClass: CFTypeRef;
        static kSecClassGenericPassword: CFTypeRef;
        static kSecAttrService: CFTypeRef;
        static kSecAttrAccount: CFTypeRef;
        static kSecValueData: CFTypeRef;
        static kSecReturnData: CFTypeRef;
        static kSecUseAuthenticationUI: CFTypeRef;
        static kSecUseAuthenticationUIFail: CFTypeRef;
        fn SecItemAdd(query: CFTypeRef, result: *mut CFTypeRef) -> i32;
        fn SecItemCopyMatching(query: CFTypeRef, result: *mut CFTypeRef) -> i32;
        fn SecItemDelete(query: CFTypeRef) -> i32;
    }
    fn query(reference: &str) -> Result<Vec<(CFType, CFType)>, String> {
        if reference.len() != 32 || !reference.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("凭据引用无效，请重新保存翻译服务设置。".into());
        }
        unsafe {
            Ok(vec![
                (CFType::wrap_under_get_rule(kSecClass), CFType::wrap_under_get_rule(kSecClassGenericPassword)),
                (CFType::wrap_under_get_rule(kSecAttrService), CFString::new("com.kaisersong.beek.api-key").as_CFType()),
                (CFType::wrap_under_get_rule(kSecAttrAccount), CFString::new(reference).as_CFType()),
                (CFType::wrap_under_get_rule(kSecUseAuthenticationUI), CFType::wrap_under_get_rule(kSecUseAuthenticationUIFail)),
            ])
        }
    }
    pub fn write(reference: &str, secret: &str) -> Result<(), String> {
        let mut pairs = query(reference)?;
        unsafe { pairs.push((CFType::wrap_under_get_rule(kSecValueData), CFData::from_buffer(secret.as_bytes()).as_CFType())); }
        let dict = CFDictionary::from_CFType_pairs(&pairs);
        let status = unsafe { SecItemAdd(dict.as_CFTypeRef(), ptr::null_mut()) };
        if status == 0 { Ok(()) } else { Err(format!("钥匙串写入失败（{status}），原配置未更改。")) }
    }
    pub fn read(reference: &str) -> Result<String, String> {
        let mut pairs = query(reference)?;
        unsafe { pairs.push((CFType::wrap_under_get_rule(kSecReturnData), CFBoolean::true_value().as_CFType())); }
        let dict = CFDictionary::from_CFType_pairs(&pairs);
        let mut result = ptr::null();
        let status = unsafe { SecItemCopyMatching(dict.as_CFTypeRef(), &mut result) };
        // Copy ownership is adopted even on an unexpected error with a non-null result.
        let owned = (!result.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(result) });
        if status != 0 { return Err(format!("钥匙串读取失败（{status}），请解锁钥匙串并重新保存翻译服务设置。")); }
        let data = owned.and_then(|v| v.downcast::<CFData>()).ok_or("钥匙串凭据格式错误。")?;
        String::from_utf8(data.bytes().to_vec()).map_err(|_| "钥匙串凭据格式错误。".into())
    }
    pub fn delete(reference: &str) {
        if let Ok(pairs) = query(reference) {
            let dict = CFDictionary::from_CFType_pairs(&pairs);
            unsafe { SecItemDelete(dict.as_CFTypeRef()); }
        }
    }
}

// Unit tests do not touch the user's login keychain.
#[cfg(test)]
mod native {
    use std::{cell::{RefCell, Cell}, collections::BTreeMap};
    thread_local! {
        static ITEMS: RefCell<BTreeMap<String,String>> = RefCell::default();
        static FAIL: Cell<u8> = const { Cell::new(0) };
    }
    pub fn fail_next(operation: u8) { FAIL.with(|v|v.set(operation)); }
    fn fails(operation: u8) -> bool { FAIL.with(|v|if v.get()==operation {v.set(0);true} else {false}) }
    pub fn count() -> usize { ITEMS.with(|v|v.borrow().len()) }
    pub fn write(reference: &str, secret: &str) -> Result<(),String> {
        if fails(1) { return Err("fixture write failure".into()); }
        ITEMS.with(|items| { items.borrow_mut().insert(reference.into(), secret.into()); });
        Ok(())
    }
    pub fn read(reference: &str) -> Result<String,String> {
        if fails(2) { return Err("fixture read failure".into()); }
        ITEMS.with(|items| items.borrow().get(reference).cloned().ok_or("凭据不存在".into()))
    }
    pub fn delete(reference: &str) { ITEMS.with(|items| { items.borrow_mut().remove(reference); }); }
}
pub use native::{delete, read, write};

#[cfg(test)]
pub use native::{count, fail_next};

#[cfg(feature = "test-fixtures")]
pub fn selftest() -> Result<(), String> {
    let mut random = [0u8;16];
    getrandom::fill(&mut random).map_err(|_| "无法生成诊断凭据引用。")?;
    let reference = random.iter().map(|b|format!("{b:02x}")).collect::<String>();
    struct Cleanup(String);
    impl Drop for Cleanup { fn drop(&mut self) { delete(&self.0); } }
    let _cleanup = Cleanup(reference.clone());
    write(&reference,"beek-public-synthetic-credential")?;
    if read(&reference)? != "beek-public-synthetic-credential" { return Err("诊断读回不一致。".into()); }
    delete(&reference);
    if read(&reference).is_ok() { return Err("诊断删除失败。".into()); }
    Ok(())
}
