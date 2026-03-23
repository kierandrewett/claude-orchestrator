/// Per-task state for streaming text messages.
#[derive(Default)]
pub struct StreamingState {
    /// The message_id of the current streaming message (to edit for continuations).
    pub current_message_id: Option<i32>,
    /// The message_id of the current tool message (to edit on completion).
    pub current_tool_message_id: Option<i32>,
    /// Accumulated text in the current message (for length checks).
    pub current_text_len: usize,
}

impl StreamingState {
    pub fn new_message(&mut self, message_id: i32) {
        self.current_message_id = Some(message_id);
        self.current_text_len = 0;
    }

    pub fn should_start_new_message(&self, new_text_len: usize) -> bool {
        self.current_message_id.is_none() || self.current_text_len + new_text_len > 3800
    }

    pub fn append(&mut self, text_len: usize) {
        self.current_text_len += text_len;
    }

    pub fn reset(&mut self) {
        self.current_message_id = None;
        self.current_tool_message_id = None;
        self.current_text_len = 0;
    }
}
