/// Per-task state for streaming text messages.
#[derive(Default)]
pub struct StreamingState {
    /// The message_id of the current streaming message (to edit for continuations).
    pub current_message_id: Option<i32>,
    /// The message_id of the current tool message (to edit on completion).
    pub current_tool_message_id: Option<i32>,
    /// Accumulated text in the current message (for length checks).
    pub current_text_len: usize,
    /// The full text of the current message (to allow editing it on completion).
    pub current_text: String,
}

impl StreamingState {
    pub fn new_message(&mut self, message_id: i32, text: &str) {
        self.current_message_id = Some(message_id);
        self.current_text_len = text.len();
        self.current_text = text.to_string();
    }

    pub fn should_start_new_message(&self, new_text_len: usize) -> bool {
        self.current_message_id.is_none() || self.current_text_len + new_text_len > 3800
    }

    pub fn append(&mut self, text: &str) {
        self.current_text_len += text.len();
        self.current_text.push_str(text);
    }

    /// Reset only the text streaming state (called when a tool starts, so the
    /// next TextOutput creates a fresh message rather than appending).
    pub fn reset_text(&mut self) {
        self.current_message_id = None;
        self.current_text_len = 0;
        self.current_text.clear();
    }

    pub fn reset(&mut self) {
        self.reset_text();
        self.current_tool_message_id = None;
    }
}
