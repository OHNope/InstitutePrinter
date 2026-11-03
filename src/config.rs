pub const STUDENT_ID: &str = "00B00000"; // Replace with your own student ID before building.

// LPD endpoint of the institute printer: lpd://172.20.83.212/kyocera
pub const PRINTER_HOST: &str = "172.20.83.212";
pub const PRINTER_QUEUE: &str = "kyocera";

const _: () = validate_student_id(STUDENT_ID);

const fn validate_student_id(id: &str) {
    let bytes = id.as_bytes();

    if bytes.len() != 8 {
        panic!("STUDENT_ID must match NNBNNNNN");
    }

    if !is_digit(bytes[0])
        || !is_digit(bytes[1])
        || bytes[2] != b'B'
        || !is_digit(bytes[3])
        || !is_digit(bytes[4])
        || !is_digit(bytes[5])
        || !is_digit(bytes[6])
        || !is_digit(bytes[7])
    {
        panic!("STUDENT_ID must match NNBNNNNN");
    }
}

const fn is_digit(byte: u8) -> bool {
    byte >= b'0' && byte <= b'9'
}
