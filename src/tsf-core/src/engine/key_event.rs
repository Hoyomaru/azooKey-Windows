use anyhow::{anyhow, Result};
use shared::windows_transport::{
    WindowsTransportInputLanguage, WindowsTransportInputStyle, WindowsTransportKeyEvent,
    WindowsTransportTextContext,
};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, GetKeyboardState, ToUnicodeEx, VIRTUAL_KEY, VK_BACK, VK_CONTROL,
        VK_DELETE, VK_DOWN, VK_ESCAPE, VK_F10, VK_F6, VK_F7, VK_F8, VK_F9, VK_LWIN, VK_MENU,
        VK_RETURN, VK_RIGHT, VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP, VK_LEFT,
    },
};

const CORE_MODIFIER_SHIFT: i32 = 1 << 0;
const CORE_MODIFIER_CONTROL: i32 = 1 << 1;
const CORE_MODIFIER_OPTION: i32 = 1 << 2;
const CORE_MODIFIER_COMMAND: i32 = 1 << 3;

/// Windows key information normalized to azooKey Desktop Core semantics.
///
/// Ordinary printable keys intentionally use Core keyCode 0. Windows virtual-key
/// numbers cannot be passed through because they overlap unrelated macOS physical
/// key codes used by UserAction (for example VK_1 == 49, while Core 49 is Space).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedKeyEvent {
    pub core_key_code: u16,
    pub characters: Option<String>,
    pub characters_ignoring_modifiers: Option<String>,
    pub modifier_flags: i32,
}

impl NormalizedKeyEvent {
    pub fn from_windows(wparam: WPARAM, lparam: LPARAM) -> Result<Self> {
        let virtual_key = VIRTUAL_KEY(wparam.0 as u16);
        let mut keyboard_state = [0_u8; 256];
        unsafe {
            GetKeyboardState(&mut keyboard_state)
                .map_err(|error| anyhow!("GetKeyboardState failed: {error}"))?;
        }

        let scan_code = ((lparam.0 as u64 >> 16) & 0xff) as u32;
        let keyboard_layout = unsafe { GetKeyboardLayout(0) };

        let characters = translate_key(
            virtual_key,
            scan_code,
            &keyboard_state,
            keyboard_layout,
        );

        let mut unmodified_state = keyboard_state;
        clear_modifier(&mut unmodified_state, VK_SHIFT);
        clear_modifier(&mut unmodified_state, VK_CONTROL);
        clear_modifier(&mut unmodified_state, VK_MENU);
        clear_modifier(&mut unmodified_state, VK_LWIN);
        clear_modifier(&mut unmodified_state, VK_RWIN);

        let characters_ignoring_modifiers = translate_key(
            virtual_key,
            scan_code,
            &unmodified_state,
            keyboard_layout,
        );

        Ok(Self {
            core_key_code: core_key_code(virtual_key),
            characters,
            characters_ignoring_modifiers,
            modifier_flags: modifier_flags(&keyboard_state),
        })
    }

    pub fn into_transport(
        self,
        event_id: u64,
        input_style: WindowsTransportInputStyle,
        input_language: WindowsTransportInputLanguage,
        activate: bool,
        context: WindowsTransportTextContext,
    ) -> WindowsTransportKeyEvent {
        WindowsTransportKeyEvent {
            event_id,
            core_key_code: self.core_key_code,
            characters: self.characters,
            characters_ignoring_modifiers: self.characters_ignoring_modifiers,
            modifier_flags: self.modifier_flags,
            input_style,
            input_language,
            activate,
            live_conversion_enabled: true,
            enable_debug_window: false,
            enable_suggestion: true,
            enable_predictive_typing: false,
            enable_typo_correction: false,
            enable_option_direct_full_width_input: false,
            type_back_slash: false,
            option_direct_input_text: None,
            visible_candidate_start_index: 0,
            context,
        }
    }
}

fn key_is_down(state: &[u8; 256], key: VIRTUAL_KEY) -> bool {
    state[key.0 as usize] & 0x80 != 0
}

fn clear_modifier(state: &mut [u8; 256], key: VIRTUAL_KEY) {
    state[key.0 as usize] = 0;
}

fn modifier_flags(state: &[u8; 256]) -> i32 {
    let mut flags = 0;
    if key_is_down(state, VK_SHIFT) {
        flags |= CORE_MODIFIER_SHIFT;
    }
    if key_is_down(state, VK_CONTROL) {
        flags |= CORE_MODIFIER_CONTROL;
    }
    if key_is_down(state, VK_MENU) {
        flags |= CORE_MODIFIER_OPTION;
    }
    if key_is_down(state, VK_LWIN) || key_is_down(state, VK_RWIN) {
        flags |= CORE_MODIFIER_COMMAND;
    }
    flags
}

fn core_key_code(key: VIRTUAL_KEY) -> u16 {
    match key {
        VK_RETURN => 0x24,
        VK_TAB => 48,
        VK_SPACE => 49,
        VK_BACK => 51,
        VK_ESCAPE => 53,
        VK_F6 => 97,
        VK_F7 => 98,
        VK_F8 => 100,
        VK_F9 => 101,
        VK_F10 => 109,
        VK_LEFT => 123,
        VK_RIGHT => 124,
        VK_DOWN => 125,
        VK_UP => 126,
        // Core deliberately treats its macOS forward-delete code as unsupported.
        VK_DELETE => 117,
        _ => 0,
    }
}

fn translate_key(
    virtual_key: VIRTUAL_KEY,
    scan_code: u32,
    keyboard_state: &[u8; 256],
    keyboard_layout: windows::Win32::UI::Input::KeyboardAndMouse::HKL,
) -> Option<String> {
    let mut buffer = [0_u16; 8];

    // Bit 2 asks modern Windows not to mutate the keyboard dead-key state.
    let written = unsafe {
        ToUnicodeEx(
            virtual_key.0 as u32,
            scan_code,
            keyboard_state,
            &mut buffer,
            4,
            keyboard_layout,
        )
    };

    if written <= 0 {
        return None;
    }

    String::from_utf16(&buffer[..written as usize])
        .ok()
        .filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_A, VK_1};

    #[test]
    fn printable_windows_virtual_keys_do_not_leak_into_core_key_codes() {
        assert_eq!(core_key_code(VK_A), 0);
        assert_eq!(core_key_code(VK_1), 0);
    }

    #[test]
    fn special_keys_map_to_desktop_core_codes() {
        assert_eq!(core_key_code(VK_RETURN), 0x24);
        assert_eq!(core_key_code(VK_SPACE), 49);
        assert_eq!(core_key_code(VK_BACK), 51);
        assert_eq!(core_key_code(VK_LEFT), 123);
        assert_eq!(core_key_code(VK_RIGHT), 124);
    }

    #[test]
    fn modifier_bits_match_desktop_core_contract() {
        let mut state = [0_u8; 256];
        state[VK_SHIFT.0 as usize] = 0x80;
        state[VK_CONTROL.0 as usize] = 0x80;
        state[VK_MENU.0 as usize] = 0x80;
        assert_eq!(
            modifier_flags(&state),
            CORE_MODIFIER_SHIFT | CORE_MODIFIER_CONTROL | CORE_MODIFIER_OPTION
        );
    }
}
