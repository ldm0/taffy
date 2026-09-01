use taffy::prelude::*;
use taffy::{
    compute_block_layout, compute_flexbox_layout, compute_grid_layout, compute_leaf_layout_with_context,
    compute_root_layout, LayoutBlockContainer, LayoutFlexboxContainer, LayoutGridContainer, LayoutInput, LayoutOutput,
    LeafLayoutContext, ResolvedAspectRatio,
};

#[derive(Clone)]
pub(super) struct TestNode {
    pub(super) style: Style,
    pub(super) scrollbar_insets: Rect<f32>,
    pub(super) resolved_aspect_ratio: Option<ResolvedAspectRatio>,
    pub(super) children: Vec<usize>,
    leaf: bool,
    measured_size: Size<f32>,
    layout: Layout,
}

impl TestNode {
    pub(super) fn container(display: Display, style: Style, scrollbar_insets: Rect<f32>) -> Self {
        Self {
            style: Style { display, ..style },
            scrollbar_insets,
            resolved_aspect_ratio: None,
            children: Vec::new(),
            leaf: false,
            measured_size: Size::ZERO,
            layout: Layout::with_order(0),
        }
    }

    pub(super) fn leaf(style: Style, measured_size: Size<f32>) -> Self {
        Self {
            style,
            scrollbar_insets: Rect::ZERO,
            resolved_aspect_ratio: None,
            children: Vec::new(),
            leaf: true,
            measured_size,
            layout: Layout::with_order(0),
        }
    }
}

pub(super) struct TestTree {
    pub(super) nodes: Vec<TestNode>,
    pub(super) layout_inputs: Vec<(usize, LayoutInput)>,
}

impl TestTree {
    pub(super) fn new(root: TestNode, child: TestNode) -> Self {
        let mut nodes = vec![root, child];
        nodes[0].children.push(1);
        Self { nodes, layout_inputs: Vec::new() }
    }

    pub(super) fn compute(&mut self, available_space: Size<AvailableSpace>) {
        compute_root_layout(self, NodeId::from(0_usize), available_space);
    }

    pub(super) fn layout(&self, index: usize) -> Layout {
        self.nodes[index].layout
    }
}

pub(super) struct ChildIter<'a>(std::slice::Iter<'a, usize>);

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

    fn get_scrollbar_insets(&self, node_id: NodeId) -> Rect<f32> {
        self.nodes[usize::from(node_id)].scrollbar_insets
    }

    fn get_resolved_aspect_ratio(&self, node_id: NodeId) -> Option<ResolvedAspectRatio> {
        let node = &self.nodes[usize::from(node_id)];
        node.resolved_aspect_ratio.or_else(|| {
            node.style.aspect_ratio.and_then(|ratio| ResolvedAspectRatio::new(ratio, node.style.box_sizing))
        })
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.nodes[usize::from(node_id)].layout = *layout;
    }

    fn compute_child_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        let index = usize::from(node_id);
        self.layout_inputs.push((index, inputs));
        let context = LeafLayoutContext::new(
            self.get_writing_mode(node_id),
            self.get_resolved_aspect_ratio(node_id),
            self.nodes[index].scrollbar_insets,
        );
        let style = self.nodes[index].style.clone();
        if self.nodes[index].leaf {
            let measured_size = self.nodes[index].measured_size;
            return compute_leaf_layout_with_context(
                inputs,
                &style,
                context,
                |_value, _basis| 0.0,
                |known, _available| Size {
                    width: known.width.unwrap_or(measured_size.width),
                    height: known.height.unwrap_or(measured_size.height),
                },
            );
        }

        match style.display {
            Display::Block | Display::FlowRoot => compute_block_layout(self, node_id, inputs, None),
            Display::Flex => compute_flexbox_layout(self, node_id, inputs),
            Display::Grid => compute_grid_layout(self, node_id, inputs),
            Display::None => unreachable!("hidden layout is not needed by these tests"),
        }
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
}

impl LayoutFlexboxContainer for TestTree {
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node_id: NodeId) -> Self::FlexboxContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_flexbox_child_style(&self, child_node_id: NodeId) -> Self::FlexboxItemStyle<'_> {
        self.get_core_container_style(child_node_id)
    }
}

impl LayoutGridContainer for TestTree {
    type GridContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: NodeId) -> Self::GridItemStyle<'_> {
        self.get_core_container_style(child_node_id)
    }
}
