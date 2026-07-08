use super::*;

mod child_responses;
mod view_scope;

pub(super) fn test_comment(id: i32, state: CommentState) -> hn_lib::app::Comment {
    hn_lib::app::Comment {
        id,
        author: format!("user{}", id),
        text: format!("comment {}", id),
        time_ago: "now".to_string(),
        state,
        depth: 0,
        deleted: false,
        child_ids: vec![id + 100],
    }
}

pub(super) fn loading(generation: u64) -> CommentState {
    CommentState::Loading { generation }
}

pub(super) fn expanded(children: Vec<hn_lib::app::Comment>) -> CommentState {
    CommentState::Expanded { children }
}

pub(super) fn ready_app() -> App {
    let mut app = App::new();
    app.set_stories(Vec::new());
    app
}
