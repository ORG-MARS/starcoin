use libc::{c_char, c_int, c_void, size_t};

#[link(name = "cryptonight", kind = "static")]
extern "C" {
    fn cryptonight_hash(
        data: *const c_char,
        hash: *const c_char,
        length: size_t,
        variant: c_int,
        height: u64,
    ) -> c_void;
}

const VARIANT: i32 = 4;
const HEIGHT: u64 = 0;

#[allow(clippy::unsound_collection_transmute)]
pub fn cryptonight_r(data: &[u8], size: usize) -> Vec<u8> {
    let hash: Vec<i8> = vec![0i8; 32];
    let data_ptr: *const c_char = data.as_ptr() as *const c_char;
    let hash_ptr: *const c_char = hash.as_ptr() as *const c_char;
    let mut hash = unsafe {
        cryptonight_hash(data_ptr, hash_ptr, size, VARIANT, HEIGHT);
        std::mem::transmute::<Vec<i8>, Vec<u8>>(hash)
    };
    hash.reverse();
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_serialize as serialize;
    use serialize::hex::FromHex;

    struct TestCase {
        input: Vec<u8>,
        output: Vec<u8>,
    }

    #[test]
    fn test_slow4() {
        let mut data = TestCase {
            input: "5468697320697320612074657374205468697320697320612074657374205468697320697320612074657374".from_hex().unwrap(),
            output: "56bbeaee6ff36e4cd22a3bef0458c57d1bce74f392b5dac62da1bc2c20fabe94".from_hex().unwrap(),
        };
        let hash = cryptonight_r(&data.input[..], data.input.len());
        data.output.reverse();
        assert_eq!(hash, data.output);
    }

    #[test]
    #[ignore]
    fn test_with_spawn() {
        use super::*;
        let input = [0x1; 76];
        loop {
            std::thread::spawn(move || {
                cryptonight_r(&input, input.len());
            });
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
