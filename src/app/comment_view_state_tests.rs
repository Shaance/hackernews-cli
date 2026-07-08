use super::{App, Comment, CommentState};

fn visible_comment_id(app: &App, index: usize) -> i32 {
    app.visible_comment_at(index).unwrap().1.id
}

fn test_comment(id: i32, author: &str, depth: usize, children: Vec<Comment>) -> Comment {
    Comment {
        id,
        author: author.to_string(),
        text: format!("Comment {}", id),
        time_ago: "now".to_string(),
        child_ids: children.iter().map(|child| child.id).collect(),
        state: if children.is_empty() {
            CommentState::Collapsed
        } else {
            CommentState::Expanded { children }
        },
        depth,
        deleted: false,
    }
}

#[test]
fn test_selected_comment_mut_updates_visible_comment_source() {
    let mut app = App::new();
    app.set_comments(vec![test_comment(1, "original", 0, Vec::new())]);

    app.selected_comment_mut().unwrap().author = "updated".to_string();

    let (_, visible_comment) = app.visible_comment_at(0).unwrap();
    assert_eq!(visible_comment.author, "updated");
    assert_eq!(app.comments[0].author, "updated");
}

#[test]
fn test_next_comment_sibling_skips_thread() {
    let child_a = test_comment(2, "child_a", 1, Vec::new());
    let child_b = test_comment(3, "child_b", 1, Vec::new());
    let top_level_a = test_comment(1, "parent", 0, vec![child_a.clone(), child_b.clone()]);
    let top_level_b = test_comment(4, "sibling", 0, Vec::new());

    let mut app = App::new();
    app.set_comments(vec![top_level_a, top_level_b.clone()]);

    // From the parent, jump to the next top-level sibling (skipping children)
    app.next_comment_sibling();
    assert_eq!(visible_comment_id(&app, app.comment_cursor), top_level_b.id);

    // From the first child, jump to its next sibling
    app.comment_cursor = 1;
    app.next_comment_sibling();
    assert_eq!(visible_comment_id(&app, app.comment_cursor), child_b.id);

    // When at the last sibling, cursor should stay in place
    app.comment_cursor = app.visible_comment_count() - 1;
    app.next_comment_sibling();
    assert_eq!(visible_comment_id(&app, app.comment_cursor), top_level_b.id);
}

#[test]
fn test_prev_comment_sibling_moves_up() {
    let child_a = test_comment(2, "child_a", 1, Vec::new());
    let child_b = test_comment(3, "child_b", 1, Vec::new());
    let top_level = test_comment(1, "parent", 0, vec![child_a.clone(), child_b.clone()]);

    let mut app = App::new();
    app.set_comments(vec![top_level]);

    app.comment_cursor = 2; // child_b
    app.prev_comment_sibling();
    assert_eq!(visible_comment_id(&app, app.comment_cursor), child_a.id);
}

#[test]
fn test_parent_comment_navigates_up_tree() {
    let child = test_comment(2, "child", 1, Vec::new());
    let parent = test_comment(1, "parent", 0, vec![child.clone()]);

    let mut app = App::new();
    app.set_comments(vec![parent]);

    // Move into child and then go to parent
    app.comment_cursor = 1;
    app.parent_comment();
    assert_eq!(visible_comment_id(&app, app.comment_cursor), 1);

    // Top-level should no-op
    app.parent_comment();
    assert_eq!(visible_comment_id(&app, app.comment_cursor), 1);
}
