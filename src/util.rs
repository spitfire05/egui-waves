/// A dead-simple cache implementation
pub struct Cache<T> {
    data: Option<T>,
}

impl<T> Cache<T> {
    pub fn new(data: T) -> Self {
        Self { data: Some(data) }
    }

    pub fn get_or_init(&mut self, init: impl FnOnce() -> T) -> &T {
        self.data.get_or_insert_with(init)
    }

    pub fn invalidate(&mut self) {
        self.data = None;
    }

    pub fn is_valid(&self) -> bool {
        self.data.is_some()
    }
}

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            data: Option::default(),
        }
    }
}

#[derive(Default)]
pub struct PlotData {
    pub waveform: Vec<[f64; 2]>,
    pub spectrum: Vec<[f64; 2]>,
}
