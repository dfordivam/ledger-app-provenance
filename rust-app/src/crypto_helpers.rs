use crate::info;
use core::default::Default;
use core::fmt;
use nanos_sdk::bindings::*;
use nanos_sdk::ecc::*;
use nanos_sdk::io::SyscallError;

use arrayvec::ArrayVec;
use bech32::*;

pub const BIP32_PATH: [u32; 5] = nanos_sdk::ecc::make_bip32_path(b"m/44'/535348'/0'/0/0");
/*
/// Helper function that derives the seed over secp256k1
pub fn bip32_derive_secp256k1(path: &[u32]) -> Result<[u8; 32], SyscallError> {
    let mut raw_key = [0u8; 32];
    nanos_sdk::ecc::bip32_derive(CurvesId::Secp256k1, path, &mut raw_key);
    Ok(raw_key)
}
*/

macro_rules! call_c_api_function {
    ($($call:tt)*) => {
        {
            let err = unsafe {
                $($call)*
            };
            if err != 0 {
                Err(SyscallError::from(err))
            } else {
                Ok(())
            }
        }
    }
}

pub fn format_signature<const K: usize>((signature, length): &([u8; K], u32)) -> Option<[u8; 64]> {
    let mut r: *const u8 = core::ptr::null();
    let mut r_len: usize = 0;
    let mut s: *const u8 = core::ptr::null();
    let mut s_len: usize = 0;

    let mut result_buffer: [u8; 64] = [0; 64];

    unsafe {
        let flag = cx_ecfp_decode_sig_der(
            signature.as_ptr(),
            *length,
            73,
            &mut r,
            &mut r_len as *mut usize as *mut u32,
            &mut s,
            &mut s_len as *mut usize as *mut u32,
        );

        // Did the decoding work?
        if flag != 1 {
            return None;
        }

        let padding1 = 32 - r_len;
        let padding2 = 32 - s_len;

        result_buffer[padding1..32].clone_from_slice(core::slice::from_raw_parts(r, r_len));
        result_buffer[32 + padding2..64].clone_from_slice(core::slice::from_raw_parts(s, s_len));
    }

    Some(result_buffer)
}

pub fn get_pubkey(path: &[u32]) -> Result<[u8; 33], CxError> {
    Ok(compress_public_key(
        Secp256k1::from_bip32(path).public_key()?,
    ))
}

/*
pub fn get_pubkey(path: &[u32]) -> Result<[u8; 33], SyscallError> {
    Secp256k1::from_bip32(path).public_key()
}

/*
#[allow(dead_code)]
pub fn get_private_key(
    path: &[u32],
) -> Result<nanos_sdk::bindings::cx_ecfp_private_key_t, SyscallError> {
    let sk = Secp256k1::from_bip32(path);
    let raw_key = bip32_derive_secp256k1(path)?;
    nanos_sdk::ecc::ec_init_key(CurvesId::Secp256k1, &raw_key)
}
*/
*/

// Public Key Hash type; update this to match the target chain's notion of an address and how to
// format one.

pub struct PKH(pub [u8; 20]);

#[allow(dead_code)]
pub fn get_pkh(key: &[u8; 33]) -> Result<PKH, SyscallError> {
    let mut temp = [0; 32];
    unsafe {
        let _len: size_t = cx_hash_sha256(key.as_ptr(), 33, temp.as_mut_ptr(), temp.len() as u32);
    }
    let mut ripemd = cx_ripemd160_t::default();
    call_c_api_function!(cx_ripemd160_init_no_throw(
        &mut ripemd as *mut cx_ripemd160_t
    ))?;
    call_c_api_function!(cx_hash_update(
        &mut ripemd as *mut cx_ripemd160_t as *mut cx_hash_t,
        temp.as_ptr(),
        temp.len() as u32
    ))?;
    let mut public_key_hash = PKH::default();
    call_c_api_function!(cx_hash_final(
        &mut ripemd as *mut cx_ripemd160_t as *mut cx_hash_t,
        public_key_hash.0[..].as_mut_ptr()
    ))?;
    Ok(public_key_hash)
}

impl Default for PKH {
    fn default() -> PKH {
        PKH(<[u8; 20]>::default())
    }
}

impl fmt::Display for PKH {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut temp = ArrayVec::<u5, 32>::new();
        self.0.write_base32(&mut temp).unwrap();
        encode_to_fmt_anycase(f, "pb", temp, Variant::Bech32).unwrap() // Don't assume that
                                                                       // this works.
    }
}

struct HexSlice<'a>(&'a [u8]);

// You can choose to implement multiple traits, like Lower and UpperHex
impl fmt::Display for HexSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            // Decide if you want to pad the value or have spaces inbetween, etc.
            write!(f, "{:02X}", byte)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Hasher(cx_sha256_s);

impl Hasher {
    pub fn new() -> Hasher {
        let mut rv = cx_sha256_s::default();
        unsafe { cx_sha256_init_no_throw(&mut rv) };
        Self(rv)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        unsafe {
            info!(
                "HASHING: {}\n{:?}",
                HexSlice(bytes),
                core::str::from_utf8(bytes)
            );
            cx_hash_update(
                &mut self.0 as *mut cx_sha256_s as *mut cx_hash_t,
                bytes.as_ptr(),
                bytes.len() as u32,
            );
        }
    }

    pub fn finalize(&mut self) -> Hash {
        let mut rv = <[u8; 32]>::default();
        unsafe {
            cx_hash_final(
                &mut self.0 as *mut cx_sha256_s as *mut cx_hash_t,
                rv.as_mut_ptr(),
            )
        };
        Hash(rv)
    }
}

pub struct Hash(pub [u8; 32]);

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{:02X}", byte)?;
        }
        Ok(())
    }
}

extern "C" {
    pub fn cx_ecfp_decode_sig_der(
        input: *const u8,
        input_len: size_t,
        max_size: size_t,
        r: *mut *const u8,
        r_len: *mut size_t,
        s: *mut *const u8,
        s_len: *mut size_t,
    ) -> u32;
}

pub fn compress_public_key(uncompressed: nanos_sdk::ecc::ECPublicKey<65, 'W'>) -> [u8; 33] {
    let mut compressed: [u8; 33] = [0; 33];

    compressed[0] = if uncompressed.pubkey[64] & 1 == 1 {
        0x03
    } else {
        0x02
    }; // "Compress" public key in place
    compressed[1..33].copy_from_slice(&uncompressed.pubkey[1..33]);
    compressed
}
