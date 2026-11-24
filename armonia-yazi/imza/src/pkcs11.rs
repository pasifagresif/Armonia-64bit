//! Kaynak: ruyap/src/akis_pkcs11.rs. Armonia Yazı imza paketine kopyalandı, 23.11.2026.
//!
//! `akis_pkcs11.rs` — PyKCS11 yerine geçen, bağımlılıksız PKCS#11 istemcisi (`libloading`).
//!
//! AKIS `akisp11.dll`, `C_Initialize`'ı YALNIZCA NULL `pInitArgs` ile kabul eder.
//! Herhangi bir `CK_C_INITIALIZE_ARGS` yapısı (`CKF_OS_LOCKING_OK` dâhil) verildiğinde
//! `CKR_ARGUMENTS_BAD` (0x07) döndürür. Bu modül `C_Initialize`'ı doğrudan NULL
//! ile çağırarak sorunu kökten çözer.

use std::ffi::c_void;
use std::fmt;
use std::os::raw::{c_uchar, c_ulong};
use std::sync::Arc;

pub type CK_ULONG = c_ulong;
pub type CK_RV = CK_ULONG;
pub type CK_BBOOL = c_uchar;

const _INVALID_LEN: CK_ULONG = CK_ULONG::MAX;

// ---- Sabitler (tüketicilerin kullandıkları) ----
pub const CKR_OK: CK_ULONG = 0x00000000;
pub const CKR_ALREADY_INIT: CK_ULONG = 0x00000191;
pub const CKU_USER: CK_ULONG = 0x00000001;
pub const CKF_SERIAL_SESSION: CK_ULONG = 0x00000004;
pub const CKF_RW_SESSION: CK_ULONG = 0x00000002;
pub const CKO_CERTIFICATE: CK_ULONG = 0x00000001;
pub const CKO_PRIVATE_KEY: CK_ULONG = 0x00000003;
pub const CKA_CLASS: CK_ULONG = 0x00000000;
pub const CKA_LABEL: CK_ULONG = 0x00000003;
pub const CKA_VALUE: CK_ULONG = 0x00000011;
pub const CKA_CERTIFICATE_TYPE: CK_ULONG = 0x00000080;
pub const CKA_KEY_TYPE: CK_ULONG = 0x00000100;
pub const CKA_ID: CK_ULONG = 0x00000102;
pub const CKK_RSA: CK_ULONG = 0x00000000;
pub const CKK_EC: CK_ULONG = 0x00000003;
pub const CKK_ECDSA: CK_ULONG = 0x00000003;
pub const CKM_RSA_PKCS: CK_ULONG = 0x00000001;
pub const CKM_ECDSA: CK_ULONG = 0x00001041;

fn is_ulong_attr(t: CK_ULONG) -> bool {
    matches!(t, CKA_CLASS | CKA_KEY_TYPE | CKA_CERTIFICATE_TYPE)
}

#[repr(C, packed(1))]
#[derive(Clone, Copy)]
pub struct CK_ATTRIBUTE {
    pub r#type: CK_ULONG,
    pub pValue: *mut c_void,
    pub ulValueLen: CK_ULONG,
}

#[repr(C, packed(1))]
#[derive(Clone, Copy)]
pub struct CK_MECHANISM {
    pub mechanism: CK_ULONG,
    pub pParameter: *mut c_void,
    pub ulParameterLen: CK_ULONG,
}

#[derive(Debug, Clone)]
pub struct PyKCS11Error {
    pub value: i64,
    pub fn_name: String,
}

impl PyKCS11Error {
    pub fn new(value: i64, fn_name: impl Into<String>) -> Self {
        Self {
            value,
            fn_name: fn_name.into(),
        }
    }
}

impl fmt::Display for PyKCS11Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = (self.value as u64) & 0xFFFF_FFFF;
        if self.fn_name.is_empty() {
            write!(f, "CKR 0x{:08X}", code)
        } else {
            write!(f, "CKR 0x{:08X} ({})", code, self.fn_name)
        }
    }
}

impl std::error::Error for PyKCS11Error {}

fn ck(fn_name: &str, rv: CK_RV) -> Result<(), PyKCS11Error> {
    if rv != CKR_OK {
        Err(PyKCS11Error::new(rv as i64, fn_name))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Mechanism {
    pub mechanism: CK_ULONG,
    pub param: Option<Vec<u8>>,
}

impl Mechanism {
    pub fn new(mechanism: CK_ULONG, param: Option<Vec<u8>>) -> Self {
        Self { mechanism, param }
    }
}

#[derive(Debug, Clone)]
pub enum TemplateVal {
    None,
    Ulong(CK_ULONG),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum AttrValue {
    Ulong(Option<CK_ULONG>),
    Bytes(Vec<u8>),
}

impl AttrValue {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            AttrValue::Bytes(b) => b,
            AttrValue::Ulong(_) => &[],
        }
    }

    pub fn as_ulong(&self) -> Option<CK_ULONG> {
        match self {
            AttrValue::Ulong(v) => *v,
            AttrValue::Bytes(_) => None,
        }
    }
}

type FnCInitialize = unsafe extern "C" fn(*mut c_void) -> CK_RV;
type FnCFinalize = unsafe extern "C" fn(*mut c_void) -> CK_RV;
type FnCGetSlotList = unsafe extern "C" fn(CK_BBOOL, *mut CK_ULONG, *mut CK_ULONG) -> CK_RV;
type FnCOpenSession =
    unsafe extern "C" fn(CK_ULONG, CK_ULONG, *mut c_void, *mut c_void, *mut CK_ULONG) -> CK_RV;
type FnCCloseSession = unsafe extern "C" fn(CK_ULONG) -> CK_RV;
type FnCLogin = unsafe extern "C" fn(CK_ULONG, CK_ULONG, *mut c_uchar, CK_ULONG) -> CK_RV;
type FnCLogout = unsafe extern "C" fn(CK_ULONG) -> CK_RV;
type FnCFindObjectsInit = unsafe extern "C" fn(CK_ULONG, *mut CK_ATTRIBUTE, CK_ULONG) -> CK_RV;
type FnCFindObjects =
    unsafe extern "C" fn(CK_ULONG, *mut CK_ULONG, CK_ULONG, *mut CK_ULONG) -> CK_RV;
type FnCFindObjectsFinal = unsafe extern "C" fn(CK_ULONG) -> CK_RV;
type FnCGetAttributeValue =
    unsafe extern "C" fn(CK_ULONG, CK_ULONG, *mut CK_ATTRIBUTE, CK_ULONG) -> CK_RV;
type FnCSignInit = unsafe extern "C" fn(CK_ULONG, *mut CK_MECHANISM, CK_ULONG) -> CK_RV;
type FnCSign =
    unsafe extern "C" fn(CK_ULONG, *const c_uchar, CK_ULONG, *mut c_uchar, *mut CK_ULONG) -> CK_RV;

struct Pkcs11Fns {
    _lib: libloading::Library,
    c_initialize: FnCInitialize,
    _c_finalize: FnCFinalize,
    c_get_slot_list: FnCGetSlotList,
    c_open_session: FnCOpenSession,
    c_close_session: FnCCloseSession,
    c_login: FnCLogin,
    c_logout: FnCLogout,
    c_find_objects_init: FnCFindObjectsInit,
    c_find_objects: FnCFindObjects,
    c_find_objects_final: FnCFindObjectsFinal,
    c_get_attribute_value: FnCGetAttributeValue,
    c_sign_init: FnCSignInit,
    c_sign: FnCSign,
}

pub struct Session {
    fns: Arc<Pkcs11Fns>,
    h: CK_ULONG,
}

impl Session {
    pub fn login(&self, pin: &str, user_type: CK_ULONG) -> Result<(), PyKCS11Error> {
        let mut pin_b = pin.as_bytes().to_vec();
        let rv = unsafe {
            (self.fns.c_login)(
                self.h,
                user_type,
                pin_b.as_mut_ptr(),
                pin_b.len() as CK_ULONG,
            )
        };
        ck("C_Login", rv)
    }

    pub fn login_user(&self, pin: &str) -> Result<(), PyKCS11Error> {
        self.login(pin, CKU_USER)
    }

    pub fn logout(&self) {
        unsafe {
            let _ = (self.fns.c_logout)(self.h);
        }
    }

    pub fn closeSession(&self) {
        unsafe {
            let _ = (self.fns.c_close_session)(self.h);
        }
    }

    pub fn findObjects(&self, template: &[(CK_ULONG, TemplateVal)]) -> Result<Vec<CK_ULONG>, PyKCS11Error> {
        enum Keep {
            Ulong(Box<CK_ULONG>),
            Bytes(Vec<u8>),
        }
        let mut keep: Vec<Keep> = Vec::with_capacity(template.len());
        let mut arr: Vec<CK_ATTRIBUTE> = Vec::with_capacity(template.len());

        for (t, v) in template {
            match v {
                TemplateVal::None => {
                    arr.push(CK_ATTRIBUTE {
                        r#type: *t,
                        pValue: std::ptr::null_mut(),
                        ulValueLen: 0,
                    });
                }
                TemplateVal::Ulong(val) => {
                    let mut boxed = Box::new(*val);
                    let ptr: *mut CK_ULONG = &mut *boxed;
                    keep.push(Keep::Ulong(boxed));
                    arr.push(CK_ATTRIBUTE {
                        r#type: *t,
                        pValue: ptr as *mut c_void,
                        ulValueLen: std::mem::size_of::<CK_ULONG>() as CK_ULONG,
                    });
                }
                TemplateVal::Bytes(vb) => {
                    if vb.is_empty() {
                        arr.push(CK_ATTRIBUTE {
                            r#type: *t,
                            pValue: std::ptr::null_mut(),
                            ulValueLen: 0,
                        });
                    } else {
                        let mut buf = vb.clone();
                        let ptr = buf.as_mut_ptr() as *mut c_void;
                        let len = buf.len() as CK_ULONG;
                        keep.push(Keep::Bytes(buf));
                        arr.push(CK_ATTRIBUTE {
                            r#type: *t,
                            pValue: ptr,
                            ulValueLen: len,
                        });
                    }
                }
            }
        }

        let rv = unsafe {
            (self.fns.c_find_objects_init)(
                self.h,
                if arr.is_empty() { std::ptr::null_mut() } else { arr.as_mut_ptr() },
                arr.len() as CK_ULONG,
            )
        };
        ck("C_FindObjectsInit", rv)?;

        let mut out = Vec::new();
        let res = (|| -> Result<(), PyKCS11Error> {
            let mut batch = [0 as CK_ULONG; 64];
            let mut cnt: CK_ULONG = 0;
            loop {
                let rv = unsafe {
                    (self.fns.c_find_objects)(self.h, batch.as_mut_ptr(), 64, &mut cnt)
                };
                ck("C_FindObjects", rv)?;
                if cnt == 0 {
                    break;
                }
                for i in 0..(cnt as usize).min(64) {
                    out.push(batch[i]);
                }
                if cnt < 64 {
                    break;
                }
            }
            Ok(())
        })();

        unsafe {
            let _ = (self.fns.c_find_objects_final)(self.h);
        }
        drop(keep);
        res?;
        Ok(out)
    }

    fn get_one(&self, obj: CK_ULONG, attr_type: CK_ULONG) -> Result<Vec<u8>, PyKCS11Error> {
        let mut a = CK_ATTRIBUTE {
            r#type: attr_type,
            pValue: std::ptr::null_mut(),
            ulValueLen: 0,
        };
        let rv = unsafe { (self.fns.c_get_attribute_value)(self.h, obj, &mut a, 1) };
        ck("C_GetAttributeValue", rv)?;
        let ln = a.ulValueLen;
        if ln == 0 || ln == _INVALID_LEN {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; ln as usize];
        a.pValue = buf.as_mut_ptr() as *mut c_void;
        let rv2 = unsafe { (self.fns.c_get_attribute_value)(self.h, obj, &mut a, 1) };
        ck("C_GetAttributeValue", rv2)?;
        let actual = (a.ulValueLen as usize).min(buf.len());
        buf.truncate(actual);
        Ok(buf)
    }

    pub fn getAttributeValue(
        &self,
        obj: CK_ULONG,
        types: &[CK_ULONG],
    ) -> Result<Vec<AttrValue>, PyKCS11Error> {
        let mut result = Vec::with_capacity(types.len());
        for &t in types {
            let raw = self.get_one(obj, t)?;
            if is_ulong_attr(t) {
                if raw.is_empty() {
                    result.push(AttrValue::Ulong(None));
                } else {
                    let mut arr = [0u8; 8];
                    let n = raw.len().min(8);
                    arr[..n].copy_from_slice(&raw[..n]);
                    result.push(AttrValue::Ulong(Some(u64::from_le_bytes(arr) as CK_ULONG)));
                }
            } else {
                result.push(AttrValue::Bytes(raw));
            }
        }
        Ok(result)
    }

    pub fn sign(
        &self,
        key: CK_ULONG,
        data: &[u8],
        mechanism: &Mechanism,
    ) -> Result<Vec<u8>, PyKCS11Error> {
        let mut mech = CK_MECHANISM {
            mechanism: mechanism.mechanism,
            pParameter: std::ptr::null_mut(),
            ulParameterLen: 0,
        };
        let rv = unsafe { (self.fns.c_sign_init)(self.h, &mut mech, key) };
        ck("C_SignInit", rv)?;

        let mut siglen: CK_ULONG = 0;
        let rv = unsafe {
            (self.fns.c_sign)(
                self.h,
                data.as_ptr(),
                data.len() as CK_ULONG,
                std::ptr::null_mut(),
                &mut siglen,
            )
        };
        ck("C_Sign", rv)?;

        let mut sig = vec![0u8; siglen as usize];
        let rv = unsafe {
            (self.fns.c_sign)(
                self.h,
                data.as_ptr(),
                data.len() as CK_ULONG,
                sig.as_mut_ptr(),
                &mut siglen,
            )
        };
        ck("C_Sign", rv)?;
        sig.truncate(siglen as usize);
        Ok(sig)
    }
}

pub struct PyKCS11Lib {
    fns: Option<Arc<Pkcs11Fns>>,
}

impl Default for PyKCS11Lib {
    fn default() -> Self {
        Self::new()
    }
}

impl PyKCS11Lib {
    pub fn new() -> Self {
        Self { fns: None }
    }

    pub fn load(&mut self, pkcs11dll_filename: &str) -> Result<(), PyKCS11Error> {
        let lib = unsafe { libloading::Library::new(pkcs11dll_filename) }.map_err(|e| {
            PyKCS11Error::new(-1, format!("DLL yuklenemedi: {} ({})", pkcs11dll_filename, e))
        })?;

        macro_rules! load_sym {
            ($name:literal, $ty:ty) => {
                unsafe { *lib.get::<$ty>($name.as_bytes()).map_err(|_| {
                    PyKCS11Error::new(-1, format!("PKCS#11 modulunde eksik fonksiyon: {}", $name))
                })? }
            };
        }

        let c_initialize = load_sym!("C_Initialize", FnCInitialize);
        let c_finalize = load_sym!("C_Finalize", FnCFinalize);
        let c_get_slot_list = load_sym!("C_GetSlotList", FnCGetSlotList);
        let c_open_session = load_sym!("C_OpenSession", FnCOpenSession);
        let c_close_session = load_sym!("C_CloseSession", FnCCloseSession);
        let c_login = load_sym!("C_Login", FnCLogin);
        let c_logout = load_sym!("C_Logout", FnCLogout);
        let c_find_objects_init = load_sym!("C_FindObjectsInit", FnCFindObjectsInit);
        let c_find_objects = load_sym!("C_FindObjects", FnCFindObjects);
        let c_find_objects_final = load_sym!("C_FindObjectsFinal", FnCFindObjectsFinal);
        let c_get_attribute_value = load_sym!("C_GetAttributeValue", FnCGetAttributeValue);
        let c_sign_init = load_sym!("C_SignInit", FnCSignInit);
        let c_sign = load_sym!("C_Sign", FnCSign);

        // AKIS YALNIZCA NULL kabul eder.
        let rv = unsafe { c_initialize(std::ptr::null_mut()) };
        if rv != CKR_OK && rv != CKR_ALREADY_INIT {
            return Err(PyKCS11Error::new(rv as i64, "C_Initialize"));
        }

        self.fns = Some(Arc::new(Pkcs11Fns {
            _lib: lib,
            c_initialize,
            _c_finalize: c_finalize,
            c_get_slot_list,
            c_open_session,
            c_close_session,
            c_login,
            c_logout,
            c_find_objects_init,
            c_find_objects,
            c_find_objects_final,
            c_get_attribute_value,
            c_sign_init,
            c_sign,
        }));
        Ok(())
    }

    pub fn getSlotList(&self, token_present: bool) -> Result<Vec<CK_ULONG>, PyKCS11Error> {
        let fns = self.fns.as_ref().ok_or_else(|| PyKCS11Error::new(-1, "DLL yuklenmedi"))?;
        let flag: CK_BBOOL = if token_present { 1 } else { 0 };
        let mut cnt: CK_ULONG = 0;
        let rv = unsafe { (fns.c_get_slot_list)(flag, std::ptr::null_mut(), &mut cnt) };
        ck("C_GetSlotList", rv)?;
        if cnt == 0 {
            return Ok(Vec::new());
        }
        let mut arr = vec![0 as CK_ULONG; cnt as usize];
        let rv = unsafe { (fns.c_get_slot_list)(flag, arr.as_mut_ptr(), &mut cnt) };
        ck("C_GetSlotList", rv)?;
        arr.truncate(cnt as usize);
        Ok(arr)
    }

    pub fn openSession(&self, slot: CK_ULONG, flags: CK_ULONG) -> Result<Session, PyKCS11Error> {
        let fns = self.fns.as_ref().ok_or_else(|| PyKCS11Error::new(-1, "DLL yuklenmedi"))?;
        let flags = flags | CKF_SERIAL_SESSION;
        let mut h: CK_ULONG = 0;
        let rv = unsafe {
            (fns.c_open_session)(slot, flags, std::ptr::null_mut(), std::ptr::null_mut(), &mut h)
        };
        ck("C_OpenSession", rv)?;
        Ok(Session {
            fns: Arc::clone(fns),
            h,
        })
    }
}
