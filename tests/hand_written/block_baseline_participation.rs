use taffy::prelude::*;
use taffy::{compute_block_layout, compute_root_layout, LayoutBlockContainer, LayoutInput, LayoutOutput, Point};

#[derive(Clone)]
struct TestNode {
    style: Style,
    children: Vec<usize>,
    leaf_output: Option<LayoutOutput>,
    contributes_baselines: bool,
    layout: Layout,
}

impl TestNode {
    fn root() -> Self {
        Self {
            style: Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                ..Style::default()
            },
            children: vec![1, 2, 3],
            leaf_output: None,
            contributes_baselines: true,
            layout: Layout::new(),
        }
    }

    fn leaf(block_size: f32, first_baseline: f32, last_baseline: f32, contributes_baselines: bool) -> Self {
        let size = Size { width: 100.0, height: block_size };
        Self {
            style: Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: length(block_size) },
                ..Style::default()
            },
            children: Vec::new(),
            leaf_output: Some(LayoutOutput::from_sizes_and_baseline_sets(
                size,
                size,
                Point { x: None, y: Some(first_baseline) },
                Point { x: None, y: Some(last_baseline) },
            )),
            contributes_baselines,
            layout: Layout::new(),
        }
    }
}

struct TestTree {
    nodes: Vec<TestNode>,
    root_output: Option<LayoutOutput>,
}

struct ChildIter<'a>(std::slice::Iter<'a, usize>);

impl Iterator for ChildIter<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied().map(NodeId::from)
    }
}

impl TraversePartialTree for TestTree {
    type ChildIter<'a> = ChildIter<'a>;

    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        ChildIter(self.nodes[usize::from(parent_node_id)].children.iter())
    }

    fn child_count(&self, parent_node_id: NodeId) -> usize {
        self.nodes[usize::from(parent_node_id)].children.len()
    }

    fn get_child_id(&self, parent_node_id: NodeId, child_index: usize) -> NodeId {
        NodeId::from(self.nodes[usize::from(parent_node_id)].children[child_index])
    }
}

impl LayoutPartialTree for TestTree {
    type CustomIdent = String;
    type CoreContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_core_container_style(&self, node_id: NodeId) -> Self::CoreContainerStyle<'_> {
        &self.nodes[usize::from(node_id)].style
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.nodes[usize::from(node_id)].layout = *layout;
    }

    fn compute_child_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        let index = usize::from(node_id);
        if let Some(output) = self.nodes[index].leaf_output {
            return output;
        }
        let output = compute_block_layout(self, node_id, inputs, None);
        if index == 0 {
            self.root_output = Some(output);
        }
        output
    }
}

impl LayoutBlockContainer for TestTree {
    type BlockContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_block_container_style(&self, node_id: NodeId) -> Self::BlockContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_block_child_style(&self, child_node_id: NodeId) -> Self::BlockItemStyle<'_> {
        self.get_core_container_style(child_node_id)
    }

    fn block_child_contributes_baselines(&self, _container_node_id: NodeId, child_node_id: NodeId) -> bool {
        self.nodes[usize::from(child_node_id)].contributes_baselines
    }
}

#[test]
fn excluded_structural_children_do_not_enter_block_baseline_sets() {
    let mut tree = TestTree {
        nodes: vec![
            TestNode::root(),
            TestNode::leaf(10.0, 5.0, 5.0, false),
            TestNode::leaf(20.0, 7.0, 15.0, true),
            TestNode::leaf(30.0, 12.0, 12.0, false),
        ],
        root_output: None,
    };

    compute_root_layout(&mut tree, NodeId::from(0_usize), Size::MAX_CONTENT);
    let output = tree.root_output.expect("root block output");

    assert_eq!(output.first_baselines, Point { x: None, y: Some(17.0) });
    assert_eq!(output.last_baselines, Point { x: None, y: Some(25.0) });
}
