use std::cell::RefCell;
use std::rc::Rc;

type ValueRef = usize;

struct Value<T> {
    dirty: bool,
    epoch: usize,
    generator: Box<(dyn FnMut(&InternalGraph<T>, Option<T>) -> T)>,
    deps: Option<Vec<usize>>,
    value: T,
}

impl<T> Value<T> {
    fn value(&self) -> &T {
        &self.value
    }
    fn set_value(&mut self, t: T) {
        self.value = t;
    }
}

#[derive(Default)]
pub struct Graph<T> {
    inner: Rc<RefCell<InternalGraph<T>>>,
}

#[derive(Default)]
struct InternalGraph<T> {
    current_execution_deps: RefCell<Option<Vec<ValueRef>>>,
    content: RefCell<Vec<RefCell<Value<T>>>>,
}

impl<T> InternalGraph<T>
where
    T: Copy,
{
    fn replace_deps(&self, new: Vec<ValueRef>) -> Option<Vec<ValueRef>> {
        self.current_execution_deps.borrow_mut().replace(new)
    }

    fn take_deps(&self) -> Option<Vec<ValueRef>> {
        self.current_execution_deps.borrow_mut().take()
    }

    fn next_ref(&self) -> ValueRef {
        self.content.borrow().len()
    }

    fn with_value<V, F>(&self, val_ref: ValueRef, f: F) -> V
    where
        F: FnOnce(&mut Value<T>) -> V,
    {
        if let Some(value_cell) = self.content.borrow().get(val_ref) {
            f(&mut value_cell.borrow_mut())
        } else {
            panic!("this should never happen")
        }
    }

    fn get(&self, val_ref: ValueRef) -> T {
        if let Some(v) = self.content.borrow().get(val_ref) {
            *v.borrow().value()
        } else {
            panic!("this should never happen")
        }
    }

    fn push_value(&self, value: Value<T>) -> ValueRef {
        let mut content = self.content.borrow_mut();
        content.push(RefCell::new(value));
        content.len() - 1
    }

    fn set_dirty(&self, val_ref: ValueRef) {
        if let Some(value) = self.content.borrow().get(val_ref) {
            let value = &mut value.borrow_mut();
            if let Some(deps) = &value.deps {
                for dep in deps {
                    self.set_dirty(*dep);
                }
            }
        }
    }
}

impl<T> Graph<T>
where
    T: Copy + Clone + std::fmt::Display + std::fmt::Debug,
{
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(InternalGraph {
                current_execution_deps: RefCell::new(None),
                content: RefCell::new(vec![]),
            })),
        }
    }

    pub fn initial(&self, initial: T) -> SettableNode<T> {
        let inner_graph = self.inner.borrow_mut();
        let new_ref = inner_graph.next_ref();
        let value = Value {
            dirty: true,
            epoch: 0,
            value: initial,
            generator: Box::new(move |g, old| {
                let mut parent_deps = g
                    .current_execution_deps
                    .try_borrow_mut()
                    .unwrap_or_else(|_| panic!("value: {:?}", old));
                if let Some(ref mut parent_deps) = *parent_deps {
                    parent_deps.push(new_ref);
                };
                if let Some(new) = old {
                    new
                } else {
                    g.get(new_ref)
                }
            }),
            deps: None,
        };
        let value_ref = inner_graph.push_value(value);
        SettableNode {
            inner: Node {
                value_ref,
                parent_graph: self.inner.clone(),
            },
        }
    }

    fn inner_borrow<V, F>(&self, f: F) -> V
    where
        F: FnOnce(&InternalGraph<T>) -> V,
    {
        f(&self.inner.borrow())
    }

    pub fn compute<F: FnMut() -> T + 'static>(&self, mut f: F) -> Node<T> {
        let (value_ref, parent_deps) =
            self.inner_borrow(|g| (g.next_ref(), g.replace_deps(vec![])));

        // compute sub-graph dependencies
        let res_value = f();
        let my_deps = if let Some(mut parent_deps) = parent_deps {
            parent_deps.push(value_ref);
            self.inner_borrow(|g| g.replace_deps(parent_deps))
        } else {
            self.inner_borrow(|g| g.take_deps())
        };

        let value: Value<T> = Value {
            dirty: true,
            epoch: 0,
            value: res_value,
            deps: my_deps,
            generator: Box::new(move |g, old| {
                let mut parent_deps = g
                    .current_execution_deps
                    .try_borrow_mut()
                    .unwrap_or_else(|_| panic!("value: {:?}", old));
                if let Some(ref mut parent_deps) = *parent_deps {
                    parent_deps.push(value_ref);
                };
                drop(parent_deps);
                f()
            }),
        };
        self.inner_borrow(move |g| g.push_value(value));
        Node {
            value_ref,
            parent_graph: self.inner.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Node<T> {
    value_ref: ValueRef,
    parent_graph: Rc<RefCell<InternalGraph<T>>>,
}

#[derive(Clone)]
pub struct SettableNode<T> {
    inner: Node<T>,
}

impl<T> SettableNode<T>
where
    T: Copy + Clone,
{
    pub fn get(&self) -> T {
        self.inner.get()
    }
    pub fn set(&self, t: T) {
        self.inner
            .parent_graph
            .borrow()
            .with_value(self.inner.value_ref, |v| v.set_value(t));
        (*self.inner.parent_graph.borrow()).set_dirty(self.inner.value_ref)
    }
}

impl<T> Node<T>
where
    T: Copy + Clone,
{
    pub fn get(&self) -> T {
        let g = &self.parent_graph.borrow();
        g.with_value(self.value_ref, |v| match v {
            Value {
                generator,
                dirty: true,
                mut value,
                ..
            } => {
                value = (generator)(g, Some(value));
                value
            }
            Value {
                dirty: false,
                value,
                ..
            } => *value,
        })
    }
}

// TODO: The nodes in the graph should really be Weak ARC'd (from the perspective of their owning Vec) in the actual array -
// only strong ARC'd by dependent nodes (so that they're dropped once the dependent nodes are gone - preventing memory leaks).

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn graph1() {
        let graph = Graph::<usize>::new();
        let graph2 = &graph;
        let a = graph2.initial(5);
        let b = graph2.initial(4);
        let c = graph.compute(move || a.get() + 6);
        let d = graph.compute(move || b.get() + c.get());
        assert_eq!(d.get(), 15);
    }

    #[test]
    fn graph2() {
        let graph = Graph::<usize>::new();
        let a = graph.initial(5);
        let a_c = a.clone();
        let b = graph.initial(4);
        let b_c = b.clone();
        let c = graph.compute(move || a.get() + 6);
        let d = graph.compute(move || b.get() + c.get());
        let e = graph.compute(move || b_c.get() * d.get());
        assert_eq!(e.get(), 60);
        a_c.set(2);
        assert_eq!(e.get(), 48);
    }
}
