use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::api::PaginationObserver;

pub struct DownloadProgress {
    bar: ProgressBar,
    message: String,
}

impl DownloadProgress {
    pub fn new(message: impl Into<String>) -> Self {
        let message = message.into();
        let bar = ProgressBar::new_spinner();

        bar.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .expect("spinner template must be valid"),
        );

        bar.set_message(message.clone());
        bar.enable_steady_tick(Duration::from_millis(100));

        Self { bar, message }
    }

    pub fn finish(&self) {
        self.bar.finish_and_clear();
    }
}

impl PaginationObserver for DownloadProgress {
    fn page_loaded(&self, loaded_records: usize, total_records: usize) {
        self.bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.cyan} {msg} \
                [{bar:32.cyan/blue}] \
                {pos}/{len} ({percent}%)",
            )
            .expect("progress template must be valid")
            .progress_chars("->-"),
        );

        self.bar.set_length(total_records as u64);
        self.bar.set_position(loaded_records as u64);
        self.bar.set_message(self.message.clone());
    }
}

impl Drop for DownloadProgress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}

pub struct ItemProgress {
    bar: ProgressBar,
}

impl ItemProgress {
    pub fn new(total: usize, message: impl Into<String>) -> Self {
        let bar = ProgressBar::new(total as u64);

        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.cyan} {msg} \
        [{bar:32.cyan/blue}] \
        {pos}/{len} ({percent}%)",
            )
            .expect("item progress template must be valid")
            .progress_chars("=>-"),
        );

        bar.set_message(message.into());
        bar.enable_steady_tick(Duration::from_millis(100));

        Self { bar }
    }

    pub fn increment(&self) {
        self.bar.inc(1);
    }

    pub fn finish(&self) {
        self.bar.finish_and_clear();
    }
}

impl Drop for ItemProgress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}
