use super::{App, Mode};

impl App {
    pub fn enter_search(&mut self) {
        self.clear_status();
        self.mode = Mode::Search;
    }

    pub fn confirm_search(&mut self) {
        self.mode = Mode::Normal;
    }

    pub fn cancel_search(&mut self) {
        self.query.clear();
        self.mode = Mode::Normal;
        self.table_state.select_first();
    }

    pub fn push_query_char(&mut self, c: char) {
        self.query.push(c);
        self.table_state.select_first();
    }

    pub fn pop_query_char(&mut self) {
        self.query.pop();
        self.table_state.select_first();
    }
}
