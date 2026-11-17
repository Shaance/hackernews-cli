//! The comment tree: depth, fold, the ancestor trail, the depth cap and focus mode.
//!
//! DESIGN.md's "Comments: reading a thread you did not start" is four mechanisms, and this
//! module is all four as state:
//!
//! - **the ancestor trail** — [`Thread::trail`], the chain from the story's top level down to
//!   the focused comment. Its last segment is the comment you are reading; the rest are its
//!   ancestors, each a re-root target and a `⌫` step.
//! - **the depth cap** — [`MAX_DEPTH`] indent levels. A comment at the cap that still has
//!   replies is [`Thread::capped`]: its children are not rows, and the row says
//!   `continue thread → N replies` instead. `Enter` re-roots there, so depth beyond the cap is
//!   carried by the trail rather than by whitespace.
//! - **focus mode** — [`Thread::dimmed`]: rows outside the focused chain use secondary ink.
//!   Nothing is hidden, so nothing has to be found again.
//! - **collapsed counts** — [`Thread::hidden_count`], derived from the tree so it cannot drift
//!   from the rows it describes.
//!
//! Re-rooting is one operation, not two: [`Thread::reroot`] makes a comment the tree's root, so
//! it draws at depth 0 and its replies follow it. `⌫` re-roots at the parent, `Enter` on a
//! `continue thread` row re-roots at that comment, and clicking a trail segment re-roots at that
//! ancestor. The trail is then just the cursor's ancestor chain, which is why it can never
//! disagree with the rows.
//!
//! The model is the desktop's own rather than `rune_kit::TreeModel`: a comment row is a wrapped
//! paragraph, and `Tree`'s virtualised rows are one fixed height (`tree/view.rs:390`), so a
//! comment tree drawn by `Tree` would truncate every comment to a single line. `TreeModel` also
//! has no re-root — its roots are fixed at insert — and re-rooting is the design's second
//! mechanism. The row drawing in `story_view` keeps the kit's conventions (indent guides, a
//! chevron lane, `level`, `expanded`), so the tree reads the same to a screen reader.

use std::collections::{HashMap, HashSet};

use hn_lib::app::Comment;

/// A source can be read only when it has content and HN has not marked it dead or deleted.
pub(crate) fn readable(comment: &Comment) -> bool {
    !comment.deleted && !comment.text.trim().is_empty()
}

/// Indent levels before the trail takes over (DESIGN.md's `MAX_DEPTH`).
///
/// A comment at depth `MAX_DEPTH - 1` that still has replies is at the cap: its children are
/// behind `continue thread →`, not behind more whitespace.
pub const MAX_DEPTH: u32 = 4;

/// Points each indent level steps in (DESIGN.md: "every indent level costs 13pt").
pub const INDENT: f32 = 13.0;

/// How much of a comment the tree draws.
///
/// HN caps a comment at 10,000 characters, which is 2,000 points of text in a 500-point pane.
/// The tree keeps the first 2,000 and says it stopped; the citation viewer still shows the whole
/// captured comment, so nothing is lost, only shortened where it is drawn.
pub const MAX_TEXT: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchStatus {
    Ready,
    Failed { ids: Vec<i32>, message: String },
}

/// One comment in the thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: i32,
    /// Unreadable parents remain as reply controls; unreadable leaves have no visible row.
    pub readable: bool,
    pub author: String,
    pub time_ago: String,
    pub text: String,
    pub links: Vec<String>,
    pub parent: Option<i32>,
    /// The replies this capture holds, in HN's order. A reply the capture does not hold is not
    /// here, so a row never claims a thread it cannot show.
    pub children: Vec<i32>,
    pub known_children: Vec<i32>,
    pub branch: BranchStatus,
}

/// One visible row: a comment and the depth it draws at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub id: i32,
    pub depth: u32,
}

/// A source has its own navigation lifetime while sharing the discussion's nodes.
#[derive(Debug, Clone)]
pub(crate) struct Navigation {
    pub cursor: Option<i32>,
    pub root: Option<i32>,
    pub folded: HashSet<i32>,
    pub focus: bool,
}

impl Navigation {
    pub fn rooted(id: i32) -> Self {
        Self { cursor: Some(id), root: Some(id), folded: HashSet::new(), focus: true }
    }
}

/// The thread: the tree, the fold set, the cursor, the re-root and focus mode.
#[derive(Debug, Clone, Default)]
pub struct Thread {
    nodes: Vec<Node>,
    index: HashMap<i32, usize>,
    roots: Vec<i32>,
    folded: HashSet<i32>,
    cursor: Option<i32>,
    /// The comment the tree is re-rooted at, drawn at depth 0. `None` is the story's top level.
    root: Option<i32>,
    focus: bool,
}

impl Thread {
    /// An empty thread: a story whose discussion has not landed.
    pub fn empty() -> Thread {
        Thread { focus: true, ..Thread::default() }
    }

    /// Build the tree from the comments a capture holds.
    ///
    /// Parent links come from each comment's `child_ids`, so a reply whose parent is missing
    /// from the capture is a root rather than a lost row. The cursor starts on the first row, so
    /// `j` has somewhere to move from the moment the discussion lands.
    pub fn from_comments(comments: &[Comment]) -> Thread {
        let mut seen = HashSet::new();
        let mut nodes: Vec<Node> = comments
            .iter().filter(|comment| seen.insert(comment.id))
            .map(|comment| {
                let readable = readable(comment);
                Node {
                    id: comment.id,
                    readable,
                    author: if readable { comment.author.clone() } else { String::new() },
                    time_ago: if readable { comment.time_ago.clone() } else { String::new() },
                    text: if readable { drawn_text(&comment.text) } else { String::new() },
                    links: if readable { comment.links.clone() } else { Vec::new() },
                    parent: None,
                    children: Vec::new(),
                    known_children: comment.child_ids.clone(),
                    branch: BranchStatus::Ready,
                }
            })
            .collect();
        let index: HashMap<i32, usize> = nodes
            .iter()
            .enumerate()
            .map(|(at, node)| (node.id, at))
            .collect();
        for at in 0..nodes.len() {
            let parent = nodes[at].id;
            for child in &nodes[at].known_children.clone() {
                if let Some(&child_at) = index.get(child) {
                    nodes[child_at].parent = Some(parent);
                    nodes[at].children.push(*child);
                }
            }
        }
        let roots: Vec<i32> = nodes
            .iter()
            .filter(|node| node.parent.is_none())
            .map(|node| node.id)
            .collect();
        let mut thread = Thread {
            nodes,
            index,
            roots,
            folded: HashSet::new(),
            cursor: None,
            root: None,
            focus: true,
        };
        thread.prune_unreadable();
        thread.cursor = thread.rows().first().map(|row| row.id);
        thread
    }

    /// Refresh known nodes and add roots without disturbing the tree being read.
    pub fn merge_comments(&mut self, roots: &[Comment], descendants: &[Comment]) {
        let selected_row = self.cursor.and_then(|id| self.visible_row(id));
        let fresh = Thread::from_comments(roots);
        let fresh_roots = fresh.roots;
        let loaded = Thread::from_comments(descendants);
        for (is_root, node) in fresh.nodes.into_iter().map(|node| (true, node))
            .chain(loaded.nodes.into_iter().map(|node| (false, node))) {
            if let Some(&at) = self.index.get(&node.id) {
                let old = &mut self.nodes[at];
                old.readable = node.readable;
                old.author = node.author;
                old.time_ago = node.time_ago;
                old.text = node.text;
                old.links = node.links;
                for id in node.known_children {
                    if !old.known_children.contains(&id) { old.known_children.push(id); }
                }
            } else if is_root && fresh_roots.contains(&node.id) {
                self.roots.push(node.id);
                self.index.insert(node.id, self.nodes.len());
                self.nodes.push(node);
            }
        }
        self.prune_unreadable();
        if self.root.is_some_and(|id| !self.displayable(id)) { self.root = None; }
        if self.cursor.is_none_or(|id| !self.displayable(id)) {
            let rows = self.rows();
            let index = selected_row.unwrap_or(0).min(rows.len().saturating_sub(1));
            self.cursor = rows.get(index).map(|row| row.id);
        }
    }

    /// Loaded descendants, including folded branches, but excluding unfetched reply IDs.
    pub fn loaded_reply_ids(&self) -> Vec<i32> {
        self.roots.iter().flat_map(|id| self.subtree(*id).into_iter().skip(1)).collect()
    }

    /// Known, potentially readable direct replies. Unfetched IDs count until resolved.
    pub fn child_count(&self, id: i32) -> usize {
        self.node(id).map_or(0, |node| node.known_children.len())
    }

    pub fn has_children(&self, id: i32) -> bool {
        self.child_count(id) > 0
    }

    fn displayable(&self, id: i32) -> bool {
        self.node(id).is_some_and(|node| node.readable || self.has_children(id))
    }

    /// Remove resolved unreadable leaves from the visible tree without losing their IDs.
    /// A group whose last child vanished is pruned in the same way, up to the root.
    fn prune_unreadable(&mut self) {
        let mut removed = HashSet::new();
        loop {
            let mut changed = false;
            for node in &self.nodes {
                if !node.readable && node.known_children.iter().all(|id| removed.contains(id)) {
                    changed |= removed.insert(node.id);
                }
            }
            if !changed { break; }
        }
        for node in &mut self.nodes {
            node.known_children.retain(|id| !removed.contains(id));
            node.children.retain(|id| !removed.contains(id));
        }
        self.roots.retain(|id| !removed.contains(id));
    }

    pub fn reply_batch(&self, id: i32, retry: bool) -> Vec<i32> {
        let Some(node) = self.node(id) else { return Vec::new(); };
        let failed = match &node.branch { BranchStatus::Ready => &[][..], BranchStatus::Failed { ids, .. } => ids };
        node.known_children.iter().filter(|id| !self.index.contains_key(id)
            && if retry { failed.contains(id) } else { !failed.contains(id) }).take(50).copied().collect()
    }

    pub fn merge_replies(&mut self, parent: i32, requested: &[i32], result: Result<Vec<Comment>, String>) {
        let Some(&at) = self.index.get(&parent) else { return; };
        let selected_row = self.cursor.and_then(|id| self.visible_row(id));
        let previous = match &self.nodes[at].branch { BranchStatus::Ready => Vec::new(), BranchStatus::Failed { ids, .. } => ids.clone() };
        let (comments, error) = match result { Ok(comments) => (comments, None), Err(message) => (Vec::new(), Some(message)) };
        for mut node in Thread::from_comments(&comments).nodes {
            if self.index.contains_key(&node.id) { continue; }
            node.parent = Some(parent);
            self.index.insert(node.id, self.nodes.len());
            self.nodes.push(node);
        }
        let known = self.nodes[at].known_children.clone();
        self.nodes[at].children = known.iter().filter(|id| self.index.contains_key(id)).copied().collect();
        let failed: Vec<_> = known.into_iter().filter(|id| !self.index.contains_key(id)
            && (previous.contains(id) || requested.contains(id))).collect();
        self.nodes[at].branch = if failed.is_empty() { BranchStatus::Ready } else {
            BranchStatus::Failed { message: error.unwrap_or_else(|| format!("{} replies unavailable", failed.len())), ids: failed }
        };
        self.prune_unreadable();
        if self.root.is_some_and(|id| !self.displayable(id)) { self.root = None; }
        if self.cursor.is_some_and(|id| !self.displayable(id)) {
            let rows = self.rows();
            let index = selected_row.unwrap_or(0).min(rows.len().saturating_sub(1));
            self.cursor = rows.get(index).map(|row| row.id);
        }
    }

    /// How many comments the capture holds.
    pub fn len(&self) -> usize {
        self.nodes.iter().filter(|node| node.readable).count()
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// How many top-level threads the story has.
    pub fn roots(&self) -> usize {
        self.roots.len()
    }

    pub fn node(&self, id: i32) -> Option<&Node> {
        self.index.get(&id).map(|&at| &self.nodes[at])
    }

    pub fn cursor(&self) -> Option<i32> {
        self.cursor
    }

    /// The comment the tree is re-rooted at, if it is re-rooted.
    pub fn root(&self) -> Option<i32> {
        self.root
    }

    pub fn focus(&self) -> bool {
        self.focus
    }

    pub fn is_folded(&self, id: i32) -> bool {
        self.folded.contains(&id)
    }

    /// The rows on screen, top to bottom: the visible roots and everything under them that is
    /// neither folded nor behind the depth cap.
    pub fn rows(&self) -> Vec<Row> {
        self.rows_with(self.root, &self.folded)
    }

    pub(crate) fn source_rows(&self, nav: &Navigation) -> Vec<Row> {
        self.rows_with(nav.root, &nav.folded)
    }

    fn rows_with(&self, root: Option<i32>, folded: &HashSet<i32>) -> Vec<Row> {
        let mut rows = Vec::new();
        match root {
            Some(root) => self.walk(root, 0, folded, &mut rows),
            None => {
                for root in &self.roots {
                    self.walk(*root, 0, folded, &mut rows);
                }
            }
        }
        rows
    }

    fn walk(&self, id: i32, depth: u32, folded: &HashSet<i32>, rows: &mut Vec<Row>) {
        rows.push(Row { id, depth });
        if folded.contains(&id) || self.capped_at(id, depth) {
            return;
        }
        for child in self.children_of(id) {
            self.walk(child, depth + 1, folded, rows);
        }
    }

    /// Reuse the thread's transitions without changing the mounted discussion's navigation.
    pub(crate) fn with_navigation<T>(&mut self, nav: &mut Navigation, f: impl FnOnce(&mut Self) -> T) -> T {
        std::mem::swap(&mut self.cursor, &mut nav.cursor);
        std::mem::swap(&mut self.root, &mut nav.root);
        std::mem::swap(&mut self.folded, &mut nav.folded);
        std::mem::swap(&mut self.focus, &mut nav.focus);
        let result = f(self);
        std::mem::swap(&mut self.cursor, &mut nav.cursor);
        std::mem::swap(&mut self.root, &mut nav.root);
        std::mem::swap(&mut self.folded, &mut nav.folded);
        std::mem::swap(&mut self.focus, &mut nav.focus);
        result
    }

    /// [`Thread::walk`] with the fold set ignored: the rows a fold would put back.
    fn walk_unfolded(&self, id: i32, depth: u32, rows: &mut Vec<Row>) {
        rows.push(Row { id, depth });
        if self.capped_at(id, depth) {
            return;
        }
        for child in self.children_of(id) {
            self.walk_unfolded(child, depth + 1, rows);
        }
    }

    /// The row `id` draws at, if it draws at all.
    pub fn visible_row(&self, id: i32) -> Option<usize> {
        self.rows().iter().position(|row| row.id == id)
    }

    /// The depth `id` draws at: 0 at the tree's root, whatever the re-root is.
    pub fn depth_of(&self, id: i32) -> Option<u32> {
        if !self.displayable(id) { return None; }
        let depth = self.tree_depth(id)?;
        match self.root {
            Some(root) => {
                if id != root && !self.is_descendant(id, root) {
                    return None;
                }
                depth.checked_sub(self.tree_depth(root)?)
            }
            None => Some(depth),
        }
    }

    /// Whether `id` is at the depth cap and still has replies, so its children are behind
    /// `continue thread →` rather than under it.
    pub fn capped(&self, id: i32) -> bool {
        self.depth_of(id)
            .is_some_and(|depth| self.capped_at(id, depth))
    }

    fn capped_at(&self, id: i32, depth: u32) -> bool {
        depth + 1 >= MAX_DEPTH && self.has_children(id)
    }

    /// How many rows a fold at `id` takes off screen: its replies as they would draw unfolded,
    /// minus the comment itself. Derived from the tree, never stored, so the count cannot drift
    /// from the rows it describes — a reply already behind a `continue thread →` is not a row,
    /// and is not counted here.
    pub fn hidden_count(&self, id: i32) -> usize {
        let Some(depth) = self.depth_of(id) else {
            return 0;
        };
        self.hidden_count_at(id, depth)
    }

    pub(crate) fn hidden_count_at(&self, id: i32, depth: u32) -> usize {
        let mut rows = Vec::new();
        for child in self.children_of(id) {
            self.walk_unfolded(child, depth + 1, &mut rows);
        }
        rows.len()
    }

    /// Every reply under `id`, including the ones behind a `continue thread →`: what the cap's
    /// label counts, because re-rooting there is what makes them rows.
    pub fn reply_count(&self, id: i32) -> usize {
        self.subtree(id).len().saturating_sub(1)
    }

    /// `id` and everything under it, depth first.
    pub fn subtree(&self, id: i32) -> Vec<i32> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(at) = stack.pop() {
            out.push(at);
            stack.extend(self.children_of(at).into_iter().rev());
        }
        out
    }

    /// The ancestor trail: the chain from the story's top level down to the focused comment.
    ///
    /// The last segment is the comment you are reading; the rest are its ancestors. It is
    /// derived from the cursor, so it cannot disagree with the rows on screen.
    pub fn trail(&self) -> Vec<i32> {
        let Some(cursor) = self.cursor else {
            return Vec::new();
        };
        let mut chain = vec![cursor];
        let mut at = cursor;
        while let Some(parent) = self.parent_of(at) {
            chain.push(parent);
            at = parent;
        }
        chain.reverse();
        chain
    }

    /// Whether `id` is the cursor, one of its ancestors, or in its subtree: the chain focus mode
    /// keeps at full contrast.
    pub fn in_chain(&self, id: i32) -> bool {
        self.in_chain_from(id, self.cursor)
    }

    pub(crate) fn in_chain_from(&self, id: i32, cursor: Option<i32>) -> bool {
        let Some(cursor) = cursor else {
            return true;
        };
        if id == cursor {
            return true;
        }
        let mut at = cursor;
        while let Some(parent) = self.parent_of(at) {
            if parent == id {
                return true;
            }
            at = parent;
        }
        self.is_descendant(id, cursor)
    }

    /// Whether focus mode dims `id`: it is on, and the row is off the chain.
    pub fn dimmed(&self, id: i32) -> bool {
        self.focus && !self.in_chain(id)
    }

    // Transitions ---------------------------------------------------------------------------

    /// Put the cursor on `id`, if it draws.
    pub fn select(&mut self, id: i32) -> bool {
        if self.visible_row(id).is_none() {
            return false;
        }
        self.cursor = Some(id);
        true
    }

    /// `j`: the next visible comment.
    ///
    /// Named for the key rather than for `Iterator`: the thread is not iterated, it is walked by
    /// a cursor that the view draws.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> bool {
        self.step(1)
    }

    /// `k`: the previous visible comment.
    pub fn prev(&mut self) -> bool {
        self.step(-1)
    }

    /// `g`: the first visible comment.
    pub fn first(&mut self) -> bool {
        let rows = self.rows();
        let Some(first) = rows.first().map(|row| row.id) else {
            return false;
        };
        if self.cursor == Some(first) {
            return false;
        }
        self.cursor = Some(first);
        true
    }

    /// `G`: the last visible comment.
    pub fn last(&mut self) -> bool {
        let rows = self.rows();
        let Some(last) = rows.last().map(|row| row.id) else {
            return false;
        };
        if self.cursor == Some(last) {
            return false;
        }
        self.cursor = Some(last);
        true
    }

    fn step(&mut self, by: isize) -> bool {
        let rows = self.rows();
        let Some(at) = self.cursor_row(&rows) else {
            return false;
        };
        let to = (at as isize + by).clamp(0, rows.len() as isize - 1) as usize;
        if to == at {
            return false;
        }
        self.cursor = Some(rows[to].id);
        true
    }

    fn cursor_row(&self, rows: &[Row]) -> Option<usize> {
        let cursor = self.cursor?;
        rows.iter().position(|row| row.id == cursor)
    }

    /// `]`: the next sibling thread.
    pub fn next_sibling(&mut self) -> bool {
        self.sibling(true)
    }

    /// `[`: the previous sibling thread.
    pub fn prev_sibling(&mut self) -> bool {
        self.sibling(false)
    }

    fn sibling(&mut self, forward: bool) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        let siblings = match self.parent_of(cursor) {
            Some(parent) => self.children_of(parent),
            None => self.roots.clone(),
        };
        let Some(at) = siblings.iter().position(|id| *id == cursor) else {
            return false;
        };
        let to = if forward {
            at + 1
        } else {
            match at.checked_sub(1) {
                Some(to) => to,
                None => return false,
            }
        };
        let Some(id) = siblings.get(to).copied() else {
            return false;
        };
        self.cursor = Some(id);
        true
    }

    /// `u`: the parent comment. A parent above the re-root is not a row, so `u` re-roots there —
    /// the same step `⌫` takes.
    pub fn to_parent(&mut self) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        let Some(parent) = self.parent_of(cursor) else {
            return false;
        };
        if self.visible_row(parent).is_some() {
            self.cursor = Some(parent);
            return true;
        }
        self.reroot(parent)
    }

    /// `h` / `c`: fold the focused comment's replies.
    pub fn fold(&mut self) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        if !self.has_children(cursor) || self.capped(cursor) {
            return false;
        }
        self.folded.insert(cursor)
    }

    /// `c`: collapse the current branch, or the nearest visible open ancestor.
    pub fn collapse(&mut self) -> bool {
        if self.cursor.is_some_and(|id| !self.children_of(id).is_empty()) && self.fold() { return true; }
        let mut at = self.cursor;
        while let Some(parent) = at.and_then(|id| self.parent_of(id)) {
            if self.visible_row(parent).is_none() { return false; }
            if self.has_children(parent) && !self.is_folded(parent) && !self.capped(parent) {
                self.cursor = Some(parent);
                return self.fold();
            }
            at = Some(parent);
        }
        false
    }

    /// `l`: unfold the focused comment, or re-root at it when it is at the cap.
    pub fn unfold(&mut self) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        if self.capped(cursor) {
            return self.reroot(cursor);
        }
        self.folded.remove(&cursor)
    }

    /// `Enter`: expand a folded thread, fold an open one, and re-root on `continue thread →`.
    pub fn activate(&mut self) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        if self.capped(cursor) {
            return self.reroot(cursor);
        }
        if self.folded.remove(&cursor) {
            return true;
        }
        if self.children_of(cursor).is_empty() {
            return false;
        }
        self.folded.insert(cursor)
    }

    /// `⌫`: pop one segment off the trail, which is re-rooting at the parent.
    pub fn pop_trail(&mut self) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        let Some(parent) = self.parent_of(cursor) else {
            return false;
        };
        self.reroot(parent)
    }

    /// Re-root the tree at `id`: it draws at depth 0, its replies follow it, and the trail gains
    /// a segment. Re-rooting at a comment unfolds it, because continuing a thread is a request
    /// to see its replies.
    pub fn reroot(&mut self, id: i32) -> bool {
        if !self.displayable(id) {
            return false;
        }
        self.root = Some(id);
        self.cursor = Some(id);
        self.folded.remove(&id);
        true
    }

    /// `f`: toggle focus mode.
    pub fn toggle_focus(&mut self) -> bool {
        self.focus = !self.focus;
        true
    }

    // Tree helpers --------------------------------------------------------------------------

    pub fn parent_of(&self, id: i32) -> Option<i32> {
        self.node(id).and_then(|node| node.parent)
    }

    pub fn children_of(&self, id: i32) -> Vec<i32> {
        self.node(id)
            .map(|node| node.children.clone())
            .unwrap_or_default()
    }

    pub fn tree_depth(&self, id: i32) -> Option<u32> {
        let mut depth = 0;
        let mut at = id;
        if !self.index.contains_key(&id) {
            return None;
        }
        while let Some(parent) = self.parent_of(at) {
            depth += 1;
            at = parent;
        }
        Some(depth)
    }

    fn is_descendant(&self, id: i32, ancestor: i32) -> bool {
        let mut at = id;
        while let Some(parent) = self.parent_of(at) {
            if parent == ancestor {
                return true;
            }
            at = parent;
        }
        false
    }
}

/// A comment's text as the tree draws it: trimmed, and bounded at [`MAX_TEXT`] with an ellipsis
/// so a row cannot run to thousands of points.
fn drawn_text(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_TEXT {
        return text.to_string();
    }
    let kept: String = text.chars().take(MAX_TEXT).collect();
    format!("{kept}\u{2026}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn thread() -> Thread {
        Thread::from_comments(&fixtures::thread())
    }

    fn ids(rows: &[Row]) -> Vec<i32> {
        rows.iter().map(|row| row.id).collect()
    }

    fn depths(rows: &[Row]) -> Vec<u32> {
        rows.iter().map(|row| row.depth).collect()
    }

    #[test]
    fn dead_deleted_and_empty_leaves_do_not_appear_in_the_discussion() {
        let mut good = fixtures::comments().remove(0);
        good.id = 1;
        good.child_ids.clear();
        let mut dead = good.clone();
        dead.id = 50521101;
        dead.author = "jheriko".into();
        dead.text = "[dead]".into();
        dead.deleted = true;
        let mut deleted = dead.clone();
        deleted.id = 50521845;
        deleted.author.clear();
        deleted.text.clear();
        let mut deleted_two = deleted.clone();
        deleted_two.id = 50521772;
        let mut blank = good.clone();
        blank.id = 50520567;
        blank.author.clear();
        blank.text = " \n\t ".into();
        let mut literal = good.clone();
        literal.id = 2;
        literal.text = "[dead]".into();
        let empty = Thread::from_comments(&[dead.clone(), deleted.clone()]);
        assert!(empty.is_empty());
        assert!(empty.rows().is_empty());
        assert_eq!(empty.cursor(), None);
        let mut thread = Thread::from_comments(&[dead, good, deleted, deleted_two, blank, literal]);
        assert_eq!(ids(&thread.rows()), vec![1, 2]);
        assert_eq!(thread.len(), 2);
        assert_eq!(thread.roots(), 2);
        assert_eq!(thread.cursor(), Some(1));
        assert!(thread.next_sibling());
        assert_eq!(thread.cursor(), Some(2));
        assert!(thread.node(50521101).is_some(), "removed items remain resolved internally");
        assert!(!thread.reroot(50521845));
    }

    #[test]
    fn unavailable_parent_stays_reachable_and_filtered_replies_are_not_failures() {
        let mut group = fixtures::comments().remove(0);
        group.id = 10;
        group.deleted = true;
        group.text.clear();
        group.links = vec!["https://example.com/hidden".into()];
        group.child_ids = vec![11, 12, 13];
        let mut good = group.clone();
        good.id = 11;
        good.deleted = false;
        good.text = "Readable reply".into();
        good.child_ids.clear();
        let mut dead = good.clone();
        dead.id = 12;
        dead.deleted = true;
        let mut thread = Thread::from_comments(&[group]);
        assert_eq!(thread.len(), 0);
        assert!(!thread.is_empty(), "an unavailable parent still offers its replies");
        assert_eq!(ids(&thread.rows()), vec![10]);
        assert!(thread.node(10).unwrap().links.is_empty());
        assert_eq!(thread.child_count(10), 3);
        assert_eq!(thread.reply_batch(10, false), vec![11, 12, 13]);
        thread.merge_replies(10, &[11, 12, 13], Err("Offline".into()));
        assert_eq!(ids(&thread.rows()), vec![10]);
        assert_eq!(thread.reply_batch(10, true), vec![11, 12, 13]);
        thread.merge_replies(10, &[11, 12, 13], Ok(vec![good, dead]));
        assert_eq!(ids(&thread.rows()), vec![10, 11]);
        assert_eq!(thread.children_of(10), vec![11]);
        assert_eq!(thread.child_count(10), 2, "the missing reply remains retryable");
        assert_eq!(thread.reply_batch(10, true), vec![13]);
        assert!(thread.reply_batch(10, false).is_empty(), "dead replies do not loop");
        assert_eq!(thread.parent_of(11), Some(10));
        assert_eq!(thread.depth_of(11), Some(1));
        assert!(thread.fold());
        assert_eq!(ids(&thread.rows()), vec![10]);
        assert!(thread.unfold());
        assert_eq!(ids(&thread.rows()), vec![10, 11]);
        let mut recovered = fixtures::comments().remove(0);
        recovered.id = 13;
        recovered.child_ids.clear();
        thread.merge_replies(10, &[13], Ok(vec![recovered]));
        assert_eq!(thread.children_of(10), vec![11, 13]);
        assert!(matches!(thread.node(10).unwrap().branch, BranchStatus::Ready));
    }

    #[test]
    fn exhausted_unavailable_group_disappears_and_reanchors_selection() {
        let mut before = fixtures::comments().remove(0);
        before.id = 1;
        before.child_ids.clear();
        let mut group = before.clone();
        group.id = 2;
        group.deleted = true;
        group.child_ids = vec![3];
        let mut after = before.clone();
        after.id = 4;
        let mut empty = before.clone();
        empty.id = 3;
        empty.text.clear();
        let mut thread = Thread::from_comments(&[before, group, after]);
        assert!(thread.select(2));
        thread.merge_replies(2, &[3], Ok(vec![empty]));
        assert_eq!(ids(&thread.rows()), vec![1, 4]);
        assert_eq!(thread.cursor(), Some(4));
        assert!(!thread.has_children(2));
        assert!(thread.reply_batch(2, true).is_empty());
        assert!(thread.node(3).is_some(), "the filtered success cannot become a retry");
    }

    #[test]
    fn exhausted_nested_groups_do_not_leave_a_dead_reroot() {
        let mut root = fixtures::comments().remove(0);
        root.id = 1;
        root.child_ids.clear();
        let mut group = root.clone();
        group.id = 2;
        group.deleted = true;
        group.child_ids = vec![3];
        let mut inner = group.clone();
        inner.id = 3;
        inner.child_ids = vec![4];
        let mut thread = Thread::from_comments(&[root, group, inner]);
        assert!(thread.reroot(3));
        let mut dead = fixtures::comments().remove(0);
        dead.id = 4;
        dead.deleted = true;
        dead.child_ids.clear();
        thread.merge_replies(3, &[4], Ok(vec![dead]));
        assert_eq!(thread.root(), None);
        assert_eq!(thread.cursor(), Some(1));
        assert_eq!(ids(&thread.rows()), vec![1]);
        assert!(!thread.has_children(2));
        assert!(!thread.has_children(3));
    }

    #[test]
    fn bounded_batches_preserve_order_and_failed_ids_do_not_block_unattempted_replies() {
        let mut root = crate::fixtures::comments().remove(0);
        root.child_ids = (1000..1123).collect();
        let mut thread = Thread::from_comments(&[root.clone(), root.clone()]);
        assert_eq!(thread.roots(), 1, "duplicate IDs never become duplicate rows");
        assert!(thread.has_children(root.id));
        let first = thread.reply_batch(root.id, false);
        assert_eq!(first, (1000..1050).collect::<Vec<_>>());
        let comments: Vec<_> = first.iter().skip(1).map(|id| { let mut c = root.clone(); c.id = *id; c.depth = 1; c.child_ids.clear(); c }).collect();
        thread.merge_replies(root.id, &first, Ok(comments));
        assert_eq!(thread.reply_batch(root.id, true), vec![1000]);
        let second = thread.reply_batch(root.id, false);
        assert_eq!(second, (1050..1100).collect::<Vec<_>>());
        thread.merge_replies(root.id, &second, Err("Offline".into()));
        assert_eq!(thread.reply_batch(root.id, false), (1100..1123).collect::<Vec<_>>());
        let mut recovered = root.clone(); recovered.id = 1000; recovered.child_ids.clear(); recovered.depth = 1;
        thread.merge_replies(root.id, &[1000], Ok(vec![recovered]));
        assert_eq!(thread.children_of(root.id)[0], 1000, "retries restore API order");
        assert_eq!(thread.reply_batch(root.id, true), second);
        assert_eq!(thread.roots(), 1);
        assert_eq!(thread.len(), 51);
    }

    #[test]
    fn the_tree_draws_depth_first_and_stops_at_the_cap() {
        let thread = thread();
        let rows = thread.rows();
        assert_eq!(
            ids(&rows),
            vec![901, 9010, 90100, 901000, 90101, 9011, 902, 903, 9030, 90300, 904, 905],
            "HN's order, depth first"
        );
        assert_eq!(depths(&rows), vec![0, 1, 2, 3, 2, 1, 0, 0, 1, 2, 0, 0]);
        // 901000 is at the cap and has replies, so its children are not rows.
        assert!(thread.capped(901000));
        assert!(!thread.capped(90100), "one level above the cap still indents");
        assert_eq!(thread.depth_of(901000), Some(3));
        assert_eq!(thread.cursor(), Some(901), "the cursor starts on the first row");
    }

    #[test]
    fn a_fold_hides_the_whole_subtree_and_counts_it() {
        let mut thread = thread();
        assert!(thread.select(9010));
        assert_eq!(
            thread.hidden_count(9010),
            3,
            "the rows the fold takes off screen: 90100, 901000 and 90101"
        );
        assert_eq!(
            thread.reply_count(9010),
            8,
            "every reply under it, including the ones behind the cap"
        );
        assert!(thread.fold());
        assert!(thread.is_folded(9010));
        assert_eq!(
            ids(&thread.rows()),
            vec![901, 9010, 9011, 902, 903, 9030, 90300, 904, 905],
            "the folded comment keeps its row, and 9011 is 901's, not 9010's"
        );
        // Folding again is not a transition; unfolding is.
        assert!(!thread.fold());
        assert!(thread.unfold());
        assert_eq!(thread.rows().len(), 12);
    }

    #[test]
    fn enter_expands_a_fold_and_folds_an_open_thread() {
        let mut thread = thread();
        thread.select(9010);
        assert!(thread.activate(), "Enter folds an open thread");
        assert!(thread.is_folded(9010));
        assert!(thread.activate(), "Enter expands a folded one");
        assert!(!thread.is_folded(9010));
        // A leaf has nothing to expand.
        thread.select(902);
        assert!(!thread.activate());
    }

    #[test]
    fn enter_on_continue_thread_reroots_and_pushes_a_trail_segment() {
        let mut thread = thread();
        thread.select(901000);
        assert_eq!(thread.trail(), vec![901, 9010, 90100, 901000]);
        assert!(thread.activate(), "Enter on the cap re-roots there");
        assert_eq!(thread.root(), Some(901000));
        assert_eq!(thread.cursor(), Some(901000));
        assert_eq!(
            ids(&thread.rows()),
            vec![901000, 9010000, 90100000, 90100001, 9010001, 9010002],
            "the re-rooted comment draws at depth 0 and its replies follow"
        );
        assert_eq!(depths(&thread.rows()), vec![0, 1, 2, 2, 1, 1]);
        assert_eq!(
            thread.trail(),
            vec![901, 9010, 90100, 901000],
            "the trail is the cursor's ancestor chain, so it did not grow twice"
        );
    }

    #[test]
    fn backspace_pops_one_trail_segment_at_a_time() {
        let mut thread = thread();
        thread.reroot(901000);
        assert!(thread.pop_trail());
        assert_eq!(thread.root(), Some(90100));
        assert_eq!(thread.cursor(), Some(90100));
        assert_eq!(thread.trail(), vec![901, 9010, 90100]);
        assert_eq!(
            ids(&thread.rows()),
            vec![90100, 901000, 9010000, 90100000, 90100001, 9010001, 9010002],
            "re-rooted at 90100, the whole subtree fits under the cap; 90101 is 9010's"
        );
        assert!(thread.pop_trail());
        assert_eq!(thread.trail(), vec![901, 9010]);
        assert!(thread.pop_trail());
        assert_eq!(thread.trail(), vec![901]);
        assert!(
            !thread.pop_trail(),
            "a top-level comment has no segment to pop"
        );
        assert_eq!(thread.root(), Some(901));
    }

    #[test]
    fn focus_mode_starts_on_and_keeps_the_chain_distinct() {
        let mut thread = thread();
        // 9010000 is behind the cap, so the tree has to be re-rooted at its parent first.
        assert!(thread.reroot(901000));
        assert!(thread.select(9010000));
        assert!(thread.focus(), "focus mode starts on");
        // The chain: the cursor, its ancestors, and its subtree.
        for on_chain in [901, 9010, 90100, 901000, 9010000, 90100000, 90100001] {
            assert!(thread.in_chain(on_chain), "{on_chain} is on the chain");
            assert!(!thread.dimmed(on_chain), "{on_chain} stays at full contrast");
        }
        // The rows off the chain: the sibling replies under the re-rooted comment.
        for off_chain in [9010001, 9010002] {
            assert!(thread.dimmed(off_chain), "{off_chain} dims");
        }
        assert!(thread.toggle_focus());
        assert!(!thread.focus());
        assert!(!thread.dimmed(9010001));
    }

    #[test]
    fn j_and_k_walk_the_visible_rows_and_stop_at_the_ends() {
        let mut thread = thread();
        assert!(thread.next());
        assert_eq!(thread.cursor(), Some(9010));
        assert!(thread.next());
        assert_eq!(thread.cursor(), Some(90100));
        assert!(thread.prev());
        assert_eq!(thread.cursor(), Some(9010));
        assert!(thread.first());
        assert_eq!(thread.cursor(), Some(901));
        assert!(!thread.prev(), "the first row has no previous");
        assert!(thread.last());
        assert_eq!(thread.cursor(), Some(905));
        assert!(!thread.next(), "the last row has no next");
    }

    #[test]
    fn brackets_walk_sibling_threads() {
        let mut thread = thread();
        thread.select(9010);
        assert!(thread.next_sibling());
        assert_eq!(thread.cursor(), Some(9011), "9011 is 9010's next sibling");
        assert!(!thread.next_sibling(), "9011 is the last reply to 901");
        assert!(thread.prev_sibling());
        assert_eq!(thread.cursor(), Some(9010));
        // At the top level the siblings are the story's threads.
        thread.select(901);
        assert!(thread.next_sibling());
        assert_eq!(thread.cursor(), Some(902));
        assert!(thread.prev_sibling());
        assert_eq!(thread.cursor(), Some(901));
    }

    #[test]
    fn u_jumps_to_the_parent_and_reroots_when_the_parent_is_above_the_root() {
        let mut thread = thread();
        thread.select(90100);
        assert!(thread.to_parent());
        assert_eq!(thread.cursor(), Some(9010));
        // Re-rooted at 9010, its parent is not a row: `u` re-roots there, as `⌫` does.
        thread.reroot(9010);
        assert!(thread.to_parent());
        assert_eq!(thread.root(), Some(901));
        assert_eq!(thread.cursor(), Some(901));
        // A top-level comment has no parent.
        assert!(!thread.to_parent());
    }

    #[test]
    fn a_reply_the_capture_does_not_hold_is_not_a_row() {
        let mut comments = fixtures::thread();
        comments.retain(|comment| comment.id != 9010000);
        let thread = Thread::from_comments(&comments);
        assert!(thread.node(9010000).is_none());
        assert!(
            !thread.children_of(901000).contains(&9010000),
            "a row never claims a thread it cannot show"
        );
        assert_eq!(
            thread.hidden_count(901000),
            2,
            "the two replies the capture still holds"
        );
        assert_eq!(
            thread.reply_count(901000),
            2,
            "and no reply it does not"
        );
    }

    #[test]
    fn an_empty_discussion_has_no_rows_and_no_cursor() {
        let mut thread = Thread::from_comments(&[]);
        assert!(thread.is_empty());
        assert!(thread.rows().is_empty());
        assert_eq!(thread.cursor(), None);
        assert!(thread.trail().is_empty());
        assert!(!thread.next());
        assert!(!thread.activate());
        assert!(!thread.pop_trail());
    }

    #[test]
    fn a_comment_longer_than_the_tree_draws_is_shortened_once() {
        let mut comments = fixtures::thread();
        comments[0].text = "x".repeat(MAX_TEXT + 10);
        let thread = Thread::from_comments(&comments);
        let text = &thread.node(901).unwrap().text;
        assert_eq!(text.chars().count(), MAX_TEXT + 1);
        assert!(text.ends_with('\u{2026}'));
        // A comment that fits is drawn as it is, trimmed.
        let mut short = fixtures::thread();
        short[0].text = "  A narrow ABI.  ".to_string();
        let thread = Thread::from_comments(&short);
        assert_eq!(thread.node(901).unwrap().text, "A narrow ABI.");
    }
}
