//! Pane group for split pane layouts.
//!
//! This module provides a tree-based layout system for terminal panes.
//! Panes can be split horizontally or vertically, creating a binary tree
//! where leaves are terminals and internal nodes are splits.
//!
//! # Structure
//!
//! ```text
//! Split (Horizontal)
//! ├── Leaf (Terminal 1)
//! └── Split (Vertical)
//!     ├── Leaf (Terminal 2)
//!     └── Leaf (Terminal 3)
//! ```
//!
//! # Usage
//!
//! ```ignore
//! // Create a new pane with a terminal
//! let terminal = cx.new(TerminalPane::new);
//! let mut panes = PaneNode::new_leaf(terminal);
//! let first_id = panes.first_leaf_id();
//!
//! // Split the pane vertically
//! let new_terminal = cx.new(TerminalPane::new);
//! if let Some(new_id) = panes.split(first_id, SplitDirection::Vertical, new_terminal) {
//!     // new_id is the UUID of the newly created pane
//! }
//!
//! // Find and remove a pane
//! panes.remove(new_id);
//! ```

use crate::terminal::TerminalPane;
use gpui::Entity;
use uuid::Uuid;

/// Direction of a split between two panes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitDirection {
    /// Side-by-side (left | right)
    Horizontal,
    /// Stacked (top / bottom)
    Vertical,
}

/// A pane group node - either a leaf (terminal) or a split (two children)
#[derive(Clone)]
pub enum PaneNode {
    Leaf {
        id: Uuid,
        terminal: Entity<TerminalPane>,
    },
    Split {
        direction: SplitDirection,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
        /// Ratio of first pane (0.0 to 1.0)
        ratio: f32,
    },
}

impl PaneNode {
    /// Creates a new leaf node containing a terminal pane.
    ///
    /// Each leaf is assigned a unique UUID for identification.
    pub fn new_leaf(terminal: Entity<TerminalPane>) -> Self {
        Self::Leaf {
            id: Uuid::new_v4(),
            terminal,
        }
    }

    /// Splits a pane into two, creating a new terminal in the second slot.
    ///
    /// Searches the tree for a pane matching `target_id`, then replaces it
    /// with a split node containing the original pane and the new terminal.
    ///
    /// Returns the UUID of the newly created pane, or `None` if the target wasn't found.
    pub fn split(
        &mut self,
        target_id: Uuid,
        direction: SplitDirection,
        new_terminal: Entity<TerminalPane>,
    ) -> Option<Uuid> {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if *id == target_id {
                    let old_terminal = terminal.clone();
                    let old_id = *id;
                    let new_id = Uuid::new_v4();

                    *self = PaneNode::Split {
                        direction,
                        first: Box::new(PaneNode::Leaf {
                            id: old_id,
                            terminal: old_terminal,
                        }),
                        second: Box::new(PaneNode::Leaf {
                            id: new_id,
                            terminal: new_terminal,
                        }),
                        ratio: 0.5,
                    };
                    Some(new_id)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => first
                .split(target_id, direction, new_terminal.clone())
                .or_else(|| second.split(target_id, direction, new_terminal)),
        }
    }

    /// Returns the UUID of the first (leftmost/topmost) leaf in the tree.
    ///
    /// Useful for setting initial focus when a tab is created or switched to.
    pub fn first_leaf_id(&self) -> Uuid {
        match self {
            PaneNode::Leaf { id, .. } => *id,
            PaneNode::Split { first, .. } => first.first_leaf_id(),
        }
    }

    /// Collects all terminal panes in the tree with their UUIDs.
    ///
    /// Returns a flat list of (id, terminal) pairs by traversing all leaves.
    pub fn all_terminals(&self) -> Vec<(Uuid, Entity<TerminalPane>)> {
        match self {
            PaneNode::Leaf { id, terminal } => vec![(*id, terminal.clone())],
            PaneNode::Split { first, second, .. } => {
                let mut result = first.all_terminals();
                result.extend(second.all_terminals());
                result
            }
        }
    }

    /// Finds a terminal pane by its UUID.
    ///
    /// Searches the tree recursively and returns the terminal entity if found.
    pub fn find_terminal(&self, target_id: Uuid) -> Option<Entity<TerminalPane>> {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if *id == target_id {
                    Some(terminal.clone())
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => first
                .find_terminal(target_id)
                .or_else(|| second.find_terminal(target_id)),
        }
    }

    /// Removes a pane from the tree by its UUID.
    ///
    /// When a leaf is removed, its parent split is replaced by the remaining sibling,
    /// effectively "promoting" it up the tree. Returns the removed node if found.
    pub fn remove(&mut self, target_id: Uuid) -> Option<PaneNode> {
        // First check what action to take without borrowing mutably
        let action = match self {
            PaneNode::Leaf { id, .. } => {
                if *id == target_id {
                    return None; // Can't remove self at this level
                }
                None
            }
            PaneNode::Split { first, second, .. } => {
                // Check if first child is the target leaf
                if let PaneNode::Leaf { id, .. } = first.as_ref() {
                    if *id == target_id {
                        Some(("promote_second", second.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
                .or_else(|| {
                    // Check if second child is the target leaf
                    if let PaneNode::Leaf { id, .. } = second.as_ref() {
                        if *id == target_id {
                            Some(("promote_first", first.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            }
        };

        if let Some((_, replacement)) = action {
            let old = std::mem::replace(self, *replacement);
            return Some(old);
        }

        // Recurse into children
        match self {
            PaneNode::Leaf { .. } => None,
            PaneNode::Split { first, second, .. } => {
                first.remove(target_id).or_else(|| second.remove(target_id))
            }
        }
    }
}
