//! Windows DPAPI 封装。
//!
//! 账号凭据（access token 等）用当前 Windows 用户的 DPAPI 主密钥加密后落盘：
//! 换用户/换机器读不到，卸载重装主密钥也会失效。比「自造 AES + 密钥文件」
//! 更安全，也省掉密钥文件管理。
//!
//! 直接声明 extern 函数，不依赖 `windows` crate，避免 API 版本漂移导致编译失败。

use anyhow::{anyhow, Result};
use std::ffi::c_void;
use std::ptr;

#[cfg(windows)]
mod imp {
    use super::*;

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    extern "system" {
        fn CryptProtectData(
            p_data_in: *const DataBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;

        fn CryptUnprotectData(
            p_data_in: *const DataBlob,
            ppsz_data_descr: *mut *mut u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;

        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
    }

    unsafe fn blob_in(bytes: &[u8]) -> DataBlob {
        DataBlob {
            cb_data: bytes.len() as u32,
            pb_data: bytes.as_ptr() as *mut u8,
        }
    }

    unsafe fn blob_out(blob: &DataBlob) -> Vec<u8> {
        if blob.cb_data == 0 || blob.pb_data.is_null() {
            return Vec::new();
        }
        std::slice::from_raw_parts(blob.pb_data, blob.cb_data as usize).to_vec()
    }

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>> {
        unsafe {
            let input = blob_in(plain);
            let mut output = DataBlob {
                cb_data: 0,
                pb_data: ptr::null_mut(),
            };
            let ok = CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            );
            if ok == 0 {
                return Err(anyhow!(
                    "DPAPI 加密失败（Windows 错误码 {}）",
                    std::io::Error::last_os_error()
                ));
            }
            let bytes = blob_out(&output);
            LocalFree(output.pb_data as *mut c_void);
            Ok(bytes)
        }
    }

    pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
        unsafe {
            let input = blob_in(cipher);
            let mut output = DataBlob {
                cb_data: 0,
                pb_data: ptr::null_mut(),
            };
            let ok = CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            );
            if ok == 0 {
                return Err(anyhow!(
                    "DPAPI 解密失败（Windows 错误码 {}）：凭据可能由其他 Windows 账号写入",
                    std::io::Error::last_os_error()
                ));
            }
            let bytes = blob_out(&output);
            LocalFree(output.pb_data as *mut c_void);
            Ok(bytes)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn protect(_plain: &[u8]) -> Result<Vec<u8>> {
        Err(anyhow!("当前仅支持 Windows（DPAPI 不可用）"))
    }

    pub fn unprotect(_cipher: &[u8]) -> Result<Vec<u8>> {
        Err(anyhow!("当前仅支持 Windows（DPAPI 不可用）"))
    }
}

/// 用 DPAPI 加密任意字节。
pub fn protect(plain: &[u8]) -> Result<Vec<u8>> {
    imp::protect(plain)
}

/// 用 DPAPI 解密任意字节。
pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
    imp::unprotect(cipher)
}
