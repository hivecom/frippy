use rand::prelude::SliceRandom;

pub(crate) struct RandomIndex {
    count: i32,
    list: Vec<i32>,
    local_index: usize,
    __initialized: bool,
}

impl RandomIndex {
    pub fn new() -> Self {
        Self {
            count: 0,
            list: Vec::new(),
            local_index: 0,
            __initialized: false,
        }
    }

    pub fn init(&mut self, count: i32) {
        if !self.__initialized {
            let list = Self::generate_list(count);

            self.count = count;
            self.list = list;
            self.__initialized = true;
        }
    }

    pub fn get(&mut self) -> Option<&i32> {
        self.local_index += 1;
        if self.__initialized && self.local_index >= self.list.len() {
            self.list = Self::generate_list(self.count);
            self.local_index = 0;
        }
        self.list.get(self.local_index)
    }

    pub fn _update_count(&mut self, new_count: i32) {
        self.count = new_count;
    }

    pub fn _force_shuffle(&mut self) {
        self.list = Self::generate_list(self.count);
    }

    fn generate_list(count: i32) -> Vec<i32> {
        let mut numbers: Vec<i32> = (1..=count).collect();
        numbers.shuffle(&mut rand::rng());
        numbers
    }
}
