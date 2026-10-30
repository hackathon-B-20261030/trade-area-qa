//! 경로 그래프와 경로 후보 (02d I-220-r, S-E7).

use mdsim_core::geom::V2;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

pub type NodeIx = u32;
pub type EdgeIx = u32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Path {
    pub nodes: Vec<NodeIx>,
    /// (간선, a→b 방향이면 true)
    pub edges: Vec<(EdgeIx, bool)>,
    pub length: f64,
}

#[derive(Clone, Debug)]
pub struct Graph {
    pub node_pos: Vec<V2>,
    pub edge_a: Vec<NodeIx>,
    pub edge_b: Vec<NodeIx>,
    pub edge_len: Vec<f64>,
    adj_start: Vec<u32>,
    adj: Vec<(EdgeIx, NodeIx)>,
}

#[derive(Copy, Clone, PartialEq)]
struct Item {
    cost: f64,
    node: NodeIx,
}

impl Eq for Item {}

impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        // 최소 힙: 비용이 작을수록, 같으면 노드 번호가 작을수록 먼저
        o.cost.total_cmp(&self.cost).then_with(|| o.node.cmp(&self.node))
    }
}

impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Graph {
    pub fn new(node_pos: Vec<V2>, edges: &[(NodeIx, NodeIx, f64)]) -> Graph {
        let n = node_pos.len();
        let mut lists: Vec<Vec<(EdgeIx, NodeIx)>> = vec![Vec::new(); n];
        for (i, &(a, b, _)) in edges.iter().enumerate() {
            lists[a as usize].push((i as EdgeIx, b));
            if a != b {
                lists[b as usize].push((i as EdgeIx, a));
            }
        }
        let mut adj_start = Vec::with_capacity(n + 1);
        let mut adj = Vec::new();
        for l in &lists {
            adj_start.push(adj.len() as u32);
            adj.extend_from_slice(l);
        }
        adj_start.push(adj.len() as u32);
        Graph { node_pos, edge_a: edges.iter().map(|e| e.0).collect(), edge_b: edges.iter().map(|e| e.1).collect(), edge_len: edges.iter().map(|e| e.2).collect(), adj_start, adj }
    }

    pub fn n_nodes(&self) -> usize {
        self.node_pos.len()
    }

    pub fn neighbors(&self, n: NodeIx) -> &[(EdgeIx, NodeIx)] {
        &self.adj[self.adj_start[n as usize] as usize..self.adj_start[n as usize + 1] as usize]
    }

    pub fn degree(&self, n: NodeIx) -> usize {
        self.neighbors(n).len()
    }

    /// 간선 e를 노드 from에서 지날 때의 방향(a→b면 true).
    pub fn forward_from(&self, e: EdgeIx, from: NodeIx) -> bool {
        self.edge_a[e as usize] == from
    }

    /// Dijkstra. `w(e, forward)`는 그 방향으로 지나는 비용(양수). 비용이 같으면 노드 번호로 정해 결정적이다.
    pub fn shortest(&self, from: NodeIx, to: NodeIx, w: &dyn Fn(EdgeIx, bool) -> f64) -> Option<Path> {
        let n = self.n_nodes();
        if from as usize >= n || to as usize >= n {
            return None;
        }
        let mut dist = vec![f64::INFINITY; n];
        let mut prev: Vec<Option<(EdgeIx, NodeIx)>> = vec![None; n];
        let mut heap = BinaryHeap::new();
        dist[from as usize] = 0.0;
        heap.push(Item { cost: 0.0, node: from });
        while let Some(Item { cost, node }) = heap.pop() {
            if cost > dist[node as usize] {
                continue;
            }
            if node == to {
                break;
            }
            for &(e, m) in self.neighbors(node) {
                let fwd = self.edge_a[e as usize] == node;
                let c = cost + w(e, fwd).max(1e-9);
                let mi = m as usize;
                if c < dist[mi] || (c == dist[mi] && prev[mi].map(|(pe, _)| e < pe).unwrap_or(false)) {
                    dist[mi] = c;
                    prev[mi] = Some((e, node));
                    heap.push(Item { cost: c, node: m });
                }
            }
        }
        if !dist[to as usize].is_finite() {
            return None;
        }
        let mut nodes = vec![to];
        let mut edges = Vec::new();
        let mut cur = to;
        while cur != from {
            let (e, p) = prev[cur as usize]?;
            edges.push((e, self.edge_a[e as usize] == p));
            nodes.push(p);
            cur = p;
        }
        nodes.reverse();
        edges.reverse();
        let length = edges.iter().map(|(e, _)| self.edge_len[*e as usize]).sum();
        Some(Path { nodes, edges, length })
    }

    /// 벌점 방식의 서로 다른 경로 후보 k개(S-E7). 찾은 경로의 간선 비용을 penalty배로 올려 다시 찾는다.
    pub fn k_paths(&self, from: NodeIx, to: NodeIx, k: usize, penalty: f64, base: &dyn Fn(EdgeIx, bool) -> f64) -> Vec<Path> {
        let mut mult = vec![1.0f64; self.edge_len.len()];
        let mut out: Vec<Path> = Vec::new();
        for _ in 0..(k * 2).max(1) {
            if out.len() >= k {
                break;
            }
            let p = match self.shortest(from, to, &|e, f| base(e, f) * mult[e as usize]) {
                Some(p) => p,
                None => break,
            };
            for (e, _) in &p.edges {
                mult[*e as usize] *= penalty;
            }
            if !out.iter().any(|q| q.edges == p.edges) {
                out.push(p);
            }
        }
        out
    }

    pub fn nearest_node(&self, p: V2) -> NodeIx {
        let mut best = (0u32, f64::INFINITY);
        for (i, q) in self.node_pos.iter().enumerate() {
            let d = q.dist(p);
            if d < best.1 {
                best = (i as u32, d);
            }
        }
        best.0
    }

    /// from에서 도달할 수 있는 노드 표시.
    pub fn reachable(&self, from: NodeIx) -> Vec<bool> {
        let mut seen = vec![false; self.n_nodes()];
        let mut stack = vec![from];
        seen[from as usize] = true;
        while let Some(n) = stack.pop() {
            for &(_, m) in self.neighbors(n) {
                if !seen[m as usize] {
                    seen[m as usize] = true;
                    stack.push(m);
                }
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0-1-2 직선과 0-3-2 우회
    fn diamond() -> Graph {
        let pos = vec![V2::new(0.0, 0.0), V2::new(10.0, 0.0), V2::new(20.0, 0.0), V2::new(10.0, 8.0)];
        Graph::new(pos, &[(0, 1, 10.0), (1, 2, 10.0), (0, 3, 12.8), (3, 2, 12.8)])
    }

    #[test]
    fn shortest_and_alternatives() {
        let g = diamond();
        let p = g.shortest(0, 2, &|e, _| g.edge_len[e as usize]).unwrap();
        assert_eq!(p.nodes, vec![0, 1, 2]);
        assert_eq!(p.edges, vec![(0, true), (1, true)]);
        let ks = g.k_paths(0, 2, 3, 1.6, &|e, _| g.edge_len[e as usize]);
        assert_eq!(ks.len(), 2, "다이아몬드에는 서로 다른 경로가 둘뿐");
        assert_eq!(ks[1].nodes, vec![0, 3, 2]);
        let back = g.shortest(2, 0, &|e, _| g.edge_len[e as usize]).unwrap();
        assert_eq!(back.edges, vec![(1, false), (0, false)]);
    }

    #[test]
    fn unreachable_is_none() {
        let pos = vec![V2::new(0.0, 0.0), V2::new(1.0, 0.0), V2::new(5.0, 5.0)];
        let g = Graph::new(pos, &[(0, 1, 1.0)]);
        assert!(g.shortest(0, 2, &|e, _| g.edge_len[e as usize]).is_none());
        assert!(g.k_paths(0, 2, 3, 1.6, &|e, _| g.edge_len[e as usize]).is_empty());
        assert_eq!(g.reachable(0), vec![true, true, false]);
    }
}
