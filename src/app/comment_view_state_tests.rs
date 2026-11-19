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
fn continuing_at_the_cap_requests_every_direct_reply() {
    let ids: Vec<_> = (100..151).collect();
    let mut records: Vec<_> = (1..6).map(|id| comment(id, "reader", &[id + 1])).collect();
    records.push(comment(6, "wide branch", &ids));
    let mut app = App::new();
    app.set_comments(records);

    assert!(app.thread.select(6));
    let (_, attempt, requested) = app.toggle_comment_replies().unwrap();
    assert_eq!(app.thread.root(), Some(6));
    assert_eq!(requested, ids);
    let mut replies: Vec<_> = ids.iter().map(|&id| comment(id, "reply", &[])).collect();
    replies[1].deleted = true;
    replies[2].text.clear();
    app.apply_comment_replies(6, attempt, Ok(replies));

    let rows = app.thread.rows();
    assert_eq!(rows.len(), 1 + 51 - 2);
    assert_eq!(rows[1].id, 100);
    assert_eq!(rows[2].id, 103);
    assert_eq!(rows.last().unwrap().depth, 1);
    assert_eq!(rows.last().unwrap().id, 150);
}

fn deep_discussion() -> App {
    let mut records: Vec<_> = (1..8).map(|id| comment(id, "reader", &[id + 1])).collect();
    records[0].child_ids.insert(0, 9);
    records.extend([comment(8, "reader", &[]), comment(9, "sibling", &[])]);
    let mut app = App::new();
    app.view_comments(42, "Discussion".into(), String::new());
    app.set_comments(records);
    app
}

#[test]
fn discussion_depth_uses_the_pane_width_and_clamps_both_ends() {
    let mut app = deep_discussion();
    for (width, expected) in [(40, 2), (65, 2), (66, 3), (71, 5), (72, 6), (240, 6)] {
        app.prepare_comment_layout(width);
        assert_eq!(app.thread.rows().last().unwrap().depth + 1, expected);
        assert!(app.thread.capped(expected as i32));
    }
}

#[test]
fn resizing_preserves_selection_or_selects_its_nearest_visible_ancestor() {
    let mut app = deep_discussion();
    app.prepare_comment_layout(120);
    assert!(app.thread.select(5));
    app.prepare_comment_layout(72);
    assert_eq!(app.thread.cursor(), Some(5));
    app.prepare_comment_layout(66);
    assert_eq!(app.thread.cursor(), Some(3));
    app.prepare_comment_layout(120);
    assert_eq!(app.thread.cursor(), Some(3));
}

#[test]
fn movement_and_fold_stay_on_drawn_rows() {
    let mut app = deep_discussion();
    app.prepare_comment_layout(64);
    for (action, expected) in [
        (App::next_comment as fn(&mut App), 9),
        (App::next_comment_sibling, 2),
        (App::next_comment, 2),
        (App::prev_comment_sibling, 9),
        (App::prev_comment, 1),
        (App::last_comment, 2),
        (App::parent_comment, 1),
        (App::first_comment, 1),
    ] {
        action(&mut app);
        assert_eq!(app.thread.cursor(), Some(expected));
    }
    assert!(app.thread.select(2));
    assert!(app.toggle_comment_replies().is_none());
    assert_eq!(app.thread.root(), Some(2));
    app.prev_comment_sibling();
    assert_eq!(app.thread.cursor(), Some(2));
    app.last_comment();
    assert_eq!(app.thread.cursor(), Some(3));
    app.collapse_current_thread();
    assert_eq!(app.thread.cursor(), Some(2));
    assert_eq!(app.visible_comment_count(), 1);
}

#[test]
fn backspace_re_roots_at_the_parent_of_an_ordinary_reply() {
    let mut app = deep_discussion();
    assert_eq!(app.thread.root(), None);
    assert!(app.thread.select(6));
    app.pop_comment_trail();
    assert_eq!(app.thread.root(), Some(5));
    assert_eq!(app.thread.cursor(), Some(5));
    assert_eq!(app.thread.trail(), [1, 2, 3, 4, 5]);

    app.pop_comment_trail();
    assert_eq!(app.thread.root(), Some(4));
    assert_eq!(app.thread.cursor(), Some(4));
}

#[test]
fn backspace_does_nothing_on_a_top_level_comment() {
    let mut app = deep_discussion();
    assert!(app.thread.select(1));
    app.pop_comment_trail();
    assert_eq!(app.thread.root(), None);
    assert_eq!(app.thread.cursor(), Some(1));
}

#[test]
fn reading_session_restores_root_trail_focus_and_reading_position() {
    let mut app = deep_discussion();
    app.prepare_comment_layout(100);
    assert!(app.thread.select(6));
    assert!(app.toggle_comment_replies().is_none());
    app.thread.toggle_focus();
    app.next_comment();
    let ranges = [(6, 0, 3), (7, 3, 14), (8, 14, 17)];
    app.update_comment_scroll(&ranges, 4);
    let anchor = app.comment_scroll_anchor();
    assert_eq!(anchor, Some((7, 0)));

    app.view_stories();
    app.view_comments(42, "Discussion".into(), String::new());
    app.prepare_comment_layout(100);
    app.update_comment_scroll(&[(6, 0, 4), (7, 4, 20), (8, 20, 23)], 4);
    assert_eq!(app.thread.root(), Some(6));
    assert_eq!(app.thread.trail(), [1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(app.thread.cursor(), Some(7));
    assert!(app.thread.focus());
    assert_eq!(app.comment_scroll_anchor(), anchor);
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
