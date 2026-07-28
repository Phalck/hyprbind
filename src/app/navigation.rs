use super::App;

impl App {
    pub fn select_next(&mut self) {
        if !self.visible().is_empty() {
            self.table_state.select_next();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.visible().is_empty() {
            self.table_state.select_previous();
        }
    }

    pub fn select_first(&mut self) {
        if !self.visible().is_empty() {
            self.table_state.select_first();
        }
    }

    pub fn select_last(&mut self) {
        if !self.visible().is_empty() {
            self.table_state.select_last();
        }
    }
}
