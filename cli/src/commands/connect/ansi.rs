use super::editor::EditorEvent;

/// State machine for parsing ANSI escape sequences.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum ControlSequenceState {
    #[default]
    None,
    Escape,
    Csi,
    CsiParams(usize),
    Ss3,
}

pub fn consume_control_sequence(state: &mut ControlSequenceState, byte: u8) -> Option<EditorEvent> {
    match state {
        ControlSequenceState::None => {
            if byte == 27 {
                *state = ControlSequenceState::Escape;
            }
            None
        }
        ControlSequenceState::Escape => {
            match byte {
                b'[' => {
                    *state = ControlSequenceState::Csi;
                    None
                }
                b'O' => {
                    *state = ControlSequenceState::Ss3;
                    None
                }
                _ => {
                    *state = ControlSequenceState::None;
                    None
                }
            }
        }
        ControlSequenceState::Csi => {
            if byte == b'~' {
                // Should not happen as params should come first
                *state = ControlSequenceState::None;
                None
            } else if byte.is_ascii_digit() || byte == b';' {
                *state = ControlSequenceState::CsiParams(if byte.is_ascii_digit() { (byte - b'0') as usize } else { 0 });
                None
            } else {
                let ev = match byte {
                    b'A' => Some(EditorEvent::HistoryUp),
                    b'B' => Some(EditorEvent::HistoryDown),
                    b'C' => Some(EditorEvent::MoveRight),
                    b'D' => Some(EditorEvent::MoveLeft),
                    b'H' => Some(EditorEvent::MoveHome),
                    b'F' => Some(EditorEvent::MoveEnd),
                    _ => None,
                };
                *state = ControlSequenceState::None;
                ev
            }
        }
        ControlSequenceState::CsiParams(val) => {
            if byte == b'~' {
                let ev = match val {
                    1 => Some(EditorEvent::MoveHome),
                    3 => Some(EditorEvent::Delete),
                    4 => Some(EditorEvent::MoveEnd),
                    _ => None,
                };
                *state = ControlSequenceState::None;
                ev
            } else if byte.is_ascii_digit() {
                // Accumulate multi-digit params
                *state = ControlSequenceState::CsiParams((*val * 10) + (byte - b'0') as usize);
                None
            } else {
                *state = ControlSequenceState::None;
                None
            }
        }
        ControlSequenceState::Ss3 => {
            let ev = match byte {
                b'A' => Some(EditorEvent::HistoryUp),
                b'B' => Some(EditorEvent::HistoryDown),
                b'C' => Some(EditorEvent::MoveRight),
                b'D' => Some(EditorEvent::MoveLeft),
                b'H' => Some(EditorEvent::MoveHome),
                b'F' => Some(EditorEvent::MoveEnd),
                _ => None,
            };
            *state = ControlSequenceState::None;
            ev
        }
    }
}
