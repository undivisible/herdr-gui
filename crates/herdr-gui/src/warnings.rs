#[derive(Default)]
pub struct Warnings(Vec<String>);

impl Warnings {
    pub fn record(&mut self, message: String) {
        if !self.0.contains(&message) {
            self.0.push(message);
        }
    }

    pub fn messages(&self) -> &[String] {
        &self.0
    }

    pub fn dismiss_all(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::Warnings;

    #[test]
    fn warnings_keep_distinct_failures_until_dismissed() {
        let mut warnings = Warnings::default();
        warnings.record("Terminal attachment failed; input discarded".into());
        assert_eq!(
            warnings.messages(),
            ["Terminal attachment failed; input discarded"]
        );
        warnings.record("Input not sent".into());
        warnings.record("Input not sent".into());
        assert_eq!(
            warnings.messages(),
            [
                "Terminal attachment failed; input discarded",
                "Input not sent"
            ]
        );
        warnings.dismiss_all();
        assert!(warnings.messages().is_empty());
    }
}
