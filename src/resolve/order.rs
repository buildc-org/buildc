use crate::diag::{Issue, Issues};
use crate::model::image::{Entry, Image};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub fn sort(image: &Image, issues: &mut Issues) -> Vec<usize> {
    let offered = |name: &str| -> Vec<usize> {
        image
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.component.as_ref().is_some_and(|m| m.answers(name)))
            .map(|(index, _)| index)
            .collect()
    };

    let n = image.entries.len();
    let mut waits_on: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (index, entry) in image.entries.iter().enumerate() {
        let Some(component) = &entry.component else {
            continue;
        };
        let hard = component
            .requires
            .iter()
            .map(|decl| decl.name.as_str())
            .chain(component.removes.iter().map(|decl| decl.name.as_str()));
        for name in hard {
            for provider in offered(name) {
                if provider != index {
                    waits_on[index].push(provider);
                }
            }
        }
        let soft = component
            .after
            .iter()
            .map(|decl| decl.name.as_str())
            .chain(component.policies.iter().copied());
        for name in soft {
            for provider in offered(name) {
                if provider != index {
                    waits_on[index].push(provider);
                }
            }
        }
        waits_on[index].sort_unstable();
        waits_on[index].dedup();
    }

    let mut blocking: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut remaining: Vec<usize> = vec![0; n];
    for (index, providers) in waits_on.iter().enumerate() {
        remaining[index] = providers.len();
        for &provider in providers {
            blocking[provider].push(index);
        }
    }

    let mut ready: BinaryHeap<Reverse<usize>> = (0..n)
        .filter(|&index| remaining[index] == 0)
        .map(Reverse)
        .collect();

    let mut order = Vec::with_capacity(n);
    while let Some(Reverse(index)) = ready.pop() {
        order.push(index);
        for &waiting in &blocking[index] {
            remaining[waiting] -= 1;
            if remaining[waiting] == 0 {
                ready.push(Reverse(waiting));
            }
        }
    }

    if order.len() < n {
        report_cycle(image, &waits_on, &remaining, issues);
        order.extend((0..n).filter(|index| remaining[*index] > 0));
    }
    order
}

pub fn apply(image: &mut Image, order: &[usize]) {
    let mut taken: Vec<Option<Entry>> = image.entries.drain(..).map(Some).collect();
    image.entries = order
        .iter()
        .filter_map(|&index| taken[index].take())
        .collect();
}

fn report_cycle(image: &Image, waits_on: &[Vec<usize>], remaining: &[usize], issues: &mut Issues) {
    let name = |index: usize| image.entries[index].path.clone();

    let mut issue = Issue::new("the component graph has a cycle", &image.src).help(
        "a requirement implies ordering, so a cycle has no build order at all; \
         drop one of the edges, or split the component that closes it",
    );
    for (index, providers) in waits_on.iter().enumerate() {
        if remaining[index] == 0 {
            continue;
        }
        let blocked: Vec<String> = providers
            .iter()
            .filter(|&&provider| remaining[provider] > 0)
            .map(|&provider| format!("`{}`", name(provider)))
            .collect();
        issue = issue.at(
            image.entries[index].span,
            format!("waits on {}", blocked.join(", ")),
        );
    }
    issues.push(issue);
}
