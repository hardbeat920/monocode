//! Port of src/shared/lib/listWindow.ts.

/// `LIST_PAGE_SIZE`: first paint of progressively mounted lists.
pub const LIST_PAGE_SIZE: usize = 32;

/// `listWindowSize`: the number of leading items to mount, including the
/// item at `required_index` when there is one.
pub fn list_window_size(total: usize, requested: usize, required_index: Option<usize>) -> usize {
    if total == 0 {
        return 0;
    }
    let required = required_index.map(|index| index + 1).unwrap_or(0);
    total.min(LIST_PAGE_SIZE.max(requested).max(required))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_0_for_an_empty_list() {
        assert_eq!(list_window_size(0, LIST_PAGE_SIZE, None), 0);
    }

    #[test]
    fn returns_the_full_list_when_it_fits_in_one_page() {
        assert_eq!(list_window_size(8, LIST_PAGE_SIZE, None), 8);
    }

    #[test]
    fn caps_the_first_page() {
        assert_eq!(list_window_size(200, LIST_PAGE_SIZE, None), LIST_PAGE_SIZE);
    }

    #[test]
    fn grows_as_more_items_are_requested() {
        assert_eq!(
            list_window_size(200, LIST_PAGE_SIZE * 2, None),
            LIST_PAGE_SIZE * 2
        );
    }

    #[test]
    fn cannot_grow_past_the_list() {
        assert_eq!(list_window_size(40, 200, None), 40);
    }

    #[test]
    fn expands_far_enough_to_include_a_required_item() {
        assert_eq!(list_window_size(200, LIST_PAGE_SIZE, Some(80)), 81);
    }
}
