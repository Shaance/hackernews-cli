use super::{App, Comment};

fn comment(id: i32, author: &str, child_ids: &[i32]) -> Comment {
    Comment {
        id,
        author: author.into(),
        text: format!("Comment {id}"),
        links: vec![],
        time_ago: "now".into(),
        child_ids: child_ids.to_vec(),
        deleted: false,
    }
}

#[test]
fn refreshed_selection_reads_updated_comment_from_thread() {
    let mut app = App::new();
    app.set_comments(vec![comment(1, "original", &[])]);
    app.begin_comment_refresh();
    app.set_comments(vec![comment(1, "updated", &[])]);

    let selected = app.thread.cursor().unwrap();
    assert_eq!(selected, 1);
    assert_eq!(app.thread.node(selected).unwrap().author, "updated");
}

#[test]
fn test_next_comment_sibling_skips_thread() {
    let mut app = App::new();
    app.set_comments(vec![
        comment(1, "parent", &[2, 3]),
        comment(2, "child_a", &[]),
        comment(3, "child_b", &[]),
        comment(4, "sibling", &[]),
    ]);

    app.next_comment_sibling();
    assert_eq!(app.thread.cursor(), Some(4));
    assert!(app.thread.select(2));
    app.next_comment_sibling();
    assert_eq!(app.thread.cursor(), Some(3));
    assert!(app.thread.select(4));
    app.next_comment_sibling();
    assert_eq!(app.thread.cursor(), Some(4));
}

#[test]
fn test_prev_comment_sibling_moves_up() {
    let mut app = App::new();
    app.set_comments(vec![
        comment(1, "parent", &[2, 3]),
        comment(2, "child_a", &[]),
        comment(3, "child_b", &[]),
    ]);

    assert!(app.thread.select(3));
    app.prev_comment_sibling();
    assert_eq!(app.thread.cursor(), Some(2));
}

#[test]
fn test_parent_comment_navigates_up_tree() {
    let mut app = App::new();
    app.set_comments(vec![comment(1, "parent", &[2]), comment(2, "child", &[])]);

    assert!(app.thread.select(2));
    app.parent_comment();
    assert_eq!(app.thread.cursor(), Some(1));
    app.parent_comment();
    assert_eq!(app.thread.cursor(), Some(1));
}

#[test]
fn collapse_from_descendant_selects_collapsing_ancestor() {
    for nested in [false, true] {
        let mut records = Vec::new();
        if nested {
            records.push(comment(5, "outer", &[10]));
        }
        records.extend([
            comment(10, "parent", &[11]),
            comment(11, "child", &[]),
            comment(20, "next", &[]),
            comment(30, "last", &[]),
        ]);
        let mut app = App::new();
        app.set_comments(records);
        app.next_comment();
        if nested {
            app.next_comment();
        }
        assert_eq!(app.thread.cursor(), Some(11));
        app.collapse_current_thread();
        assert_eq!(app.thread.cursor(), Some(10));
        assert_eq!(app.visible_comment_count(), if nested { 4 } else { 3 });
        assert!(app.thread.is_folded(10));
    }
}

#[test]
fn tui_expands_the_whole_branch_without_a_visible_level_cap() {
    let ids: Vec<_> = (100..151).collect();
    let mut records: Vec<_> = (1..7).map(|id| comment(id, "reader", &[id + 1])).collect();
    records.push(comment(7, "wide branch", &ids));
    let mut app = App::new();
    app.set_comments(records);

    assert!(app.thread.select(7));
    let (_, attempt, requested) = app.toggle_comment_replies().unwrap();
    assert_eq!(requested, ids);
    let mut replies: Vec<_> = ids.iter().map(|&id| comment(id, "reply", &[])).collect();
    replies[1].deleted = true;
    replies[2].text.clear();
    app.apply_comment_replies(7, attempt, Ok(replies));

    let rows = app.thread.rows();
    assert_eq!(rows.len(), 7 + 51 - 2);
    assert_eq!(rows[7].id, 100);
    assert_eq!(rows[8].id, 103);
    assert_eq!(rows.last().unwrap().depth, 7);
    assert_eq!(rows.last().unwrap().id, 150);
}

#[test]
fn refresh_includes_loaded_branches_that_are_folded() {
    let mut app = App::new();
    app.set_comments(vec![
        comment(1, "open", &[2]),
        comment(2, "reply", &[]),
        comment(3, "folded", &[4]),
        comment(4, "cached", &[]),
    ]);
    assert!(app.thread.select(3));
    assert!(
        app.toggle_comment_replies().is_none(),
        "folding cached replies makes no request"
    );
    assert!(!app.comment_replies_open(3));

    let (_, branches) = app.begin_comment_refresh();
    assert_eq!(branches, [1, 3].into_iter().collect());
}
