use std::cell::RefCell;
use std::rc::Rc;

type ValueRef = usize;

struct BaseValue<T> {
    dirty: bool,
    epoch: usize,
    value: T,
    deps: Vec<usize>,
}

struct ResultValue<T> {
    dirty: bool,
    epoch: usize,
    generator: Box<(dyn FnMut(&InternalGraph<T>) -> T)>,
    deps: Option<Vec<usize>>,
    value: T,
}

enum Value<T> {
    Res(ResultValue<T>),
    Base(BaseValue<T>),
}

impl<'a, T> Value<T> {
    fn value(&self) -> &T {
        match self {
            Self::Res(v) => &v.value,
            Self::Base(v) => &v.value,
        }
    }
    fn set_dirty(&mut self, value: bool) {
        match self {
            Self::Res(v) => v.dirty = value,
            Self::Base(v) => v.dirty = value,
        }
    }
    fn is_dirty(&self) -> bool {
        match self {
            Self::Res(v) => v.dirty,
            Self::Base(v) => v.dirty,
        }
    }
    fn set_value(&mut self, t: T) {
        match self {
            Self::Res(v) => v.value = t,
            Self::Base(v) => v.value = t,
        }
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

impl<T> InternalGraph<T> {
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

    fn push_value(&self, value: Value<T>) -> ValueRef {
        let mut content = self.content.borrow_mut();
        content.push(RefCell::new(value));
        content.len() - 1
    }

    fn set_dirty(&self, val_ref: ValueRef) {}

    // fn get_value(&self) -> &'a mut Value<'a, T> {

    // }
}

impl<T> Graph<T>
where
    T: Copy + Clone,
{
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(InternalGraph {
                current_execution_deps: RefCell::new(None),
                content: RefCell::new(vec![]),
            })),
        }
    }

    pub fn initial<'a, 'b: 'a>(&'b self, initial: T) -> Node<T> {
        let value = Value::Base(BaseValue {
            dirty: true,
            epoch: 0,
            value: initial,
            deps: vec![],
        });
        let inner_graph = self.inner.borrow_mut();
        let value_ref = inner_graph.push_value(value);
        Node {
            value_ref,
            parent_graph: self.inner.clone(),
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

        let value: Value<T> = Value::Res(ResultValue {
            dirty: true,
            epoch: 0,
            value: res_value,
            deps: my_deps,
            generator: Box::new(move |g| {
                // let is_dirty = self.inner_borrow(|g| {
                //     // this is being called in a compilation context
                //     if let Some(ref mut parent_deps) = &mut g.current_execution_deps {
                //         parent_deps.push(value_ref);
                //     };
                //     // TODO: this can be done with unsafe since no two indices will be borrowed at the same time
                //     g.content[value_ref].borrow().is_dirty()
                // });
                // if is_dirty {
                //     let generator = self.inner_borrow(|g| {
                //         let value_cell = &g.content[value_ref];
                //         let value: &mut Value<T> = &mut value_cell.borrow_mut();
                //         if let Value::Res(ref mut value) = value {
                //             value.generator
                //         } else {
                //             panic!("This should never happen");
                //         }
                //     });
                //     let new_value: T = generator();
                //     self.inner_borrow(|g| {
                //         let value_cell = &g.content[value_ref];
                //         let value: &mut Value<T> = &mut value_cell.borrow_mut();
                //         value.set_value(new_value);
                //         value.set_dirty(false);
                //     });
                //     new_value
                // } else {
                //     self.inner_borrow(|g| {
                //         let value_cell = &g.content[value_ref];
                //         let value: &mut Value<T> = &mut value_cell.borrow();
                //         *value.value()
                //     })
                // }
                f()
            }),
        });
        self.inner_borrow(move |g| g.push_value(value));
        Node {
            value_ref,
            parent_graph: self.inner.clone(),
        }
    }
}
pub struct Node<T> {
    value_ref: ValueRef,
    parent_graph: Rc<RefCell<InternalGraph<T>>>,
}

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
            Value::Res(ref mut v) => (v.generator)(g),
            Value::Base(ref mut v) => v.value,
        })
    }
    // pub fn and(&self, other: &Node<T>) -> Node<T> {
    //     let value = Value::Res(ResultValue {
    //         epoch: 0,
    //         value: None,
    //         deps: vec![self.value_ref, other.value_ref],
    //     });
    //     let mut graph = self.parent_graph.borrow_mut();
    //     let value_ref = graph.content.len();
    //     graph.content.push(value);
    //     Node {
    //         value_ref,
    //         parent_graph: self.parent_graph.clone(),
    //     }
    // }
}

trait Query {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn graph() {
        let graph = Graph::<usize>::new();
        let graph2 = &graph;
        let a = graph2.initial(5);
        let b = graph2.initial(4);
        let c = graph.compute(move || a.get() + 6);
        let d = graph.compute(move || c.get() + b.get());
        assert_eq!(d.get(), 15);
        // let mut v1 = graph2.initial(1);
        // let mut v2 = graph.initial(2);
        // let mut v1v2 = graph.compute(move || v1() + v2());
        // let mut result = graph.compute(move || v1v2() + 5);
        // let res_val = result();
        // assert_eq!(res_val, 3);
    }
}
