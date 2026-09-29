/// Maps Windows virtual-key codes into the platform-neutral key-code values
/// currently understood by azooKey Desktop's UserAction layer.
///
/// The Core still uses the historical macOS key-code numbers for non-printable
/// keys. Keeping that translation at the Windows adapter boundary prevents the
/// shared conversion engine from learning about Win32 VK values.
pub fn core_key_code_from_vk(vk: u16) -> u16 {
    match vk {
        0x0D => 0x24, // VK_RETURN -> Return
        0x09 => 48,   // VK_TAB
        0x20 => 49,   // VK_SPACE
        0x08 => 51,   // VK_BACK -> Delete/Backspace
        0x1B => 53,   // VK_ESCAPE

        0x75 => 97,  // VK_F6
        0x76 => 98,  // VK_F7
        0x77 => 100, // VK_F8
        0x78 => 101, // VK_F9
        0x79 => 109, // VK_F10

        0x25 => 123, // VK_LEFT
        0x27 => 124, // VK_RIGHT
        0x28 => 125, // VK_DOWN
        0x26 => 126, // VK_UP

        0x6F => 0x4B, // VK_DIVIDE
        0x6C => 0x5F, // VK_SEPARATOR
        0x6E => 0x41, // VK_DECIMAL

        // Number row. UserAction currently uses the macOS physical key-code
        // values to distinguish numeric candidate selection.
        0x31 => 18,
        0x32 => 19,
        0x33 => 20,
        0x34 => 21,
        0x35 => 23,
        0x36 => 22,
        0x37 => 26,
        0x38 => 28,
        0x39 => 25,
        0x30 => 29,

        // Printable keys do not need a physical code: UserAction falls through
        // to the translated character. Zero is therefore a safe neutral value.
        _ => 0,
    }
}

pub const MODIFIER_SHIFT: i32 = 1 << 0;
pub const MODIFIER_CONTROL: i32 = 1 << 1;
pub const MODIFIER_ALT: i32 = 1 << 2;
pub const MODIFIER_WINDOWS: i32 = 1 << 3;

pub fn core_modifier_flags(
    shift: bool,
    control: bool,
    alt: bool,
    windows_key: bool,
) -> i32 {
    let mut flags = 0;
    if shift {
        flags |= MODIFIER_SHIFT;
    }
    if control {
        flags |= MODIFIER_CONTROL;
    }
    if alt {
        // Core calls this modifier "option"; Alt is the Windows equivalent.
        flags |= MODIFIER_ALT;
    }
    if windows_key {
        // Core calls this modifier "command". This is only used for routing
        // host shortcuts and should normally be passed through to the app.
        flags |= MODIFIER_WINDOWS;
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_control_keys_to_core_codes() {
        assert_eq!(core_key_code_from_vk(0x0D), 0x24);
        assert_eq!(core_key_code_from_vk(0x08), 51);
        assert_eq!(core_key_code_from_vk(0x1B), 53);
        assert_eq!(core_key_code_from_vk(0x25), 123);
        assert_eq!(core_key_code_from_vk(0x27), 124);
        assert_eq!(core_key_code_from_vk(0x28), 125);
        assert_eq!(core_key_code_from_vk(0x26), 126);
    }

    #[test]
    fn maps_function_keys() {
        assert_eq!(core_key_code_from_vk(0x75), 97);
        assert_eq!(core_key_code_from_vk(0x76), 98);
        assert_eq!(core_key_code_from_vk(0x77), 100);
        assert_eq!(core_key_code_from_vk(0x78), 101);
        assert_eq!(core_key_code_from_vk(0x79), 109);
    }

    #[test]
    fn maps_number_row_for_candidate_selection() {
        let expected = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
        for (digit, expected_code) in expected.into_iter().enumerate() {
            let vk = if digit == 0 { 0x30 } else { 0x30 + digit as u16 };
            assert_eq!(core_key_code_from_vk(vk), expected_code);
        }
    }

    #[test]
    fn builds_core_modifier_mask() {
        assert_eq!(core_modifier_flags(true, true, false, false), 0b0011);
        assert_eq!(core_modifier_flags(false, false, true, false), 0b0100);
        assert_eq!(core_modifier_flags(false, false, false, true), 0b1000);
    }
}
