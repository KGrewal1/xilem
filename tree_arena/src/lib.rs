// Copyright 2024 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! This module will eventually be factored out into a separate crate.
//!
//! In the meantime, we intentionally don't make the types in this module part of
//! our public API, but still implement methods that a standalone crate would have.
//!
//! The types defined in this module don't *actually* implement an arena. They use
//! 100% safe code, which has a significant performance overhead. The final version
//! will use an arena and unsafe code, but should have the exact same exported API as
//! this module.

#[cfg(not(feature = "safe_tree"))]
mod tree_arena_unsafe;
#[cfg(not(feature = "safe_tree"))]
pub use tree_arena_unsafe::*;

mod tree_arena_safe;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_tree_test() {
        let mut tree: TreeArena<char> = TreeArena::new();
        let mut roots = tree.root_token_mut();
        roots.insert_child(1_u64, 'a');
        roots.insert_child(2_u64, 'b');
        let mut child_1 = roots.get_child_mut(1_u64).expect("No child 1 found");
        child_1.children.insert_child(3_u64, 'c');

        let mut child_3 = child_1
            .children
            .get_child_mut(3_u64)
            .expect("No child 3 found");
        child_3.children.insert_child(4_u64, 'd');

        let child_2 = tree.find(2_u64).expect("No child 2 found");
        let child_4 = child_2.children.find(4_u64);
        assert!(
            child_4.is_none(),
            "Child 4 should not be descended from Child 2"
        );
    }

    #[test]
    fn arena_tree_removal_test() {
        let mut tree: TreeArena<char> = TreeArena::new();
        let mut roots = tree.root_token_mut();
        roots.insert_child(1_u64, 'a');
        roots.insert_child(2_u64, 'b');
        let mut child_1 = roots.get_child_mut(1_u64).expect("No child 1 found");
        child_1.children.insert_child(3_u64, 'c');

        let mut child_3 = child_1
            .children
            .get_child_mut(3_u64)
            .expect("No child 3 found");
        child_3.children.insert_child(4_u64, 'd');

        let child_3_removed = child_1
            .children
            .remove_child(3_u64)
            .expect("No child 3 found");
        assert_eq!(child_3_removed, 'c', "Expect removal of node 3");

        let no_child_3_removed = child_1.children.remove_child(3_u64);
        assert!(no_child_3_removed.is_none(), "Child 3 was not removed");
    }

    #[test]
    #[should_panic(expected = "Key already present")]
    fn arena_tree_duplicate_insertion() {
        let mut tree: TreeArena<char> = TreeArena::new();
        let mut roots = tree.root_token_mut();
        roots.insert_child(1_u64, 'a');
        roots.insert_child(1_u64, 'b');
    }
}
