#[derive(Default)]
pub struct SkillReport {
    pub messages: Vec<String>,
    pub has_issue: bool,
}

impl SkillReport {
    pub fn add_issue(&mut self, msg: String) {
        self.has_issue = true;
        self.messages.push(msg);
    }

    pub fn add_note(&mut self, msg: String) {
        self.messages.push(msg);
    }
}
