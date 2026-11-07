//! What pressing Enter at the end of a list item does.

#[derive(Debug, PartialEq, Eq)]
pub enum ListAction {
    /// Start the next line with this marker.
    Insert(String),
    /// The item was empty: end the list by removing its line.
    Remove,
    None,
}

/// `line` is the text of the line that was just ended, without the newline.
pub fn continue_list(line: &str) -> ListAction {
    let mut chars = line.chars();
    if let (Some(marker @ ('-' | '+' | '*' | '•' | '☐' | '☑')), Some(' ')) = (chars.next(), chars.next()) {
        if line.chars().count() == 2 {
            return ListAction::Remove;
        }
        // a ticked item carries on as a fresh, unticked one
        return ListAction::Insert(format!("{} ", if marker == '☑' { '☐' } else { marker }));
    }

    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if (1..=8).contains(&digits) && line[digits..].starts_with(". ") {
        if line.chars().count() == digits + 2 {
            return ListAction::Remove;
        }
        if let Ok(n) = line[..digits].parse::<u64>() {
            return ListAction::Insert(format!("{}. ", n + 1));
        }
    }
    ListAction::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ListAction::*;

    #[test]
    fn lists_carry_on_or_end() {
        assert_eq!(continue_list("• uno"), Insert("• ".into()));
        assert_eq!(continue_list("- uno"), Insert("- ".into()));
        assert_eq!(continue_list("☑ fatto"), Insert("☐ ".into()));
        assert_eq!(continue_list("☐ "), Remove);
        assert_eq!(continue_list("9. nove"), Insert("10. ".into()));
        assert_eq!(continue_list("10. "), Remove);
        assert_eq!(continue_list("testo normale"), None);
        assert_eq!(continue_list("2024 anno"), None);
        assert_eq!(continue_list(""), None);
    }
}
