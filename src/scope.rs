use crate::Graph;
use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Deref;
use std::ops::DerefMut;
use std::rc::Rc;

/// Joins left and right using the separator.  If either left or right is the
/// empty string, the separator is left out.
fn join(sep: &str, left: &str, right: &str) -> String {
    match (left, right) {
        ("", _) => right.to_string(),
        (_, "") => left.to_string(),
        _ => format!("{}{}{}", left, sep, right),
    }
}

// TODO: Include other with_* functions
/// A `Scope` object represents a set of related TensorFlow ops that have the
/// same properties such as a common name prefix.
///
/// This type currently requires the `experimental_training` feature.
///
/// A Scope object is a container for TensorFlow Op properties. Op constructors
/// get a Scope object as a mandatory first argument and the constructed op
/// acquires the properties in the object.
///
/// A simple example:
///
/// ```
/// # use tensorflow::Scope;
/// # use tensorflow::Tensor;
/// # use tensorflow::ops;
/// let mut root = Scope::new_root_scope();
/// let c1 = ops::constant(Tensor::new(&[1, 2]).with_values(&[1, 1])?, &mut root)?;
/// let c2 = ops::constant(Tensor::new(&[2, 1]).with_values(&[41, 1])?, &mut root)?;
/// let m = ops::mat_mul(c1, c2, &mut root)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Scope hierarchy
///
/// The Scope class provides various `with_*` functions that create a new scope.
/// The new scope typically has one property changed while other properties are
/// inherited from the parent scope.
/// `new_sub_scope(name)` method appends `name` to the prefix of names for ops
/// created within the scope, and `with_op_name()` changes the suffix which
/// otherwise defaults to the type of the op.
///
/// Name examples:
///
/// ```
/// # use tensorflow::DataType;
/// # use tensorflow::Scope;
/// # use tensorflow::Shape;
/// # use tensorflow::Tensor;
/// # use tensorflow::Variable;
/// # use tensorflow::ops;
/// let mut root = Scope::new_root_scope();
/// let mut linear = root.new_sub_scope("linear");
/// let w = Variable::builder()
///   .const_initial_value(
///     Tensor::new(&[2, 2])
///       .with_values(&[0.0f32, 0.0, 0.0, 0.0])?)
///   .build(&mut linear.with_op_name("W"))?;
/// assert_eq!(w.name(), "linear/W");
/// let b = Variable::builder()
///   .const_initial_value(
///     Tensor::new(&[2])
///       .with_values(&[0.0f32, 0.0])?)
///   .build(&mut linear.with_op_name("b"))?;
/// assert_eq!(b.name(), "linear/b");
/// let x = ops::constant(
///   Tensor::new(&[2, 2])
///     .with_values(&[1.0f32, 2.0, 3.0, 4.0])?,
///    &mut linear)?;
/// assert_eq!(x.name()?, "linear/Const");
/// let m = ops::mat_mul(x, w.output().clone(), &mut linear)?;
/// assert_eq!(m.name()?, "linear/MatMul");
/// let r = ops::bias_add(m, b.output().clone(), &mut linear)?;
/// assert_eq!(r.name()?, "linear/BiasAdd");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Scope lifetime
///
/// A new scope is created by calling `Scope::new_root_scope`. This creates some
/// resources that are shared by all the child scopes that inherit from this
/// scope, directly or transitively. For instance, a new scope creates a new
/// Graph object to which operations are added when the new scope or its
/// children are used by an Op constructor.
#[derive(Debug)]
pub struct Scope {
    graph: Rc<RefCell<Graph>>,
    name: String,
    children_names: Rc<RefCell<HashSet<String>>>,
    op_name: String,
    op_names: Rc<RefCell<HashMap<String, i32>>>,
}

impl Scope {
    /// Return a new scope.
    /// This creates a new graph and all operations constructed in this graph
    /// should use the returned object as the "root" scope.
    pub fn new_root_scope() -> Scope {
        Scope {
            graph: Rc::new(RefCell::new(Graph::new())),
            name: "".to_string(),
            children_names: Rc::new(RefCell::new(HashSet::new())),
            op_name: "".to_string(),
            op_names: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Adds a suffix if necessary to create a unique subscope name.
    fn uniquify(&self, name: &str) -> String {
        let refcell: &RefCell<_> = self.children_names.borrow();
        let mut set = refcell.borrow_mut();
        if set.insert(name.to_string()) {
            return name.to_string();
        }
        let mut i = 1;
        loop {
            let unique_name = format!("{}_{}", &name, i);
            if set.insert(unique_name.clone()) {
                return unique_name;
            }
            i += 1;
        }
    }

    /// Return a new scope. Ops created with this scope will have
    /// `name/child_scope_name` as the prefix. The actual name will be unique
    /// in the current scope. All other properties are inherited from the current
    /// scope. If `child_scope_name` is empty, the `/` is elided.
    pub fn new_sub_scope(&self, name: &str) -> Scope {
        let self_name: &str = &self.name;
        let (new_name, copy_names) = match (self_name, name) {
            (_, "") => (self.name.clone(), true),
            ("", _) => (self.uniquify(name), false),
            _ => (format!("{}/{}", self.name, self.uniquify(name)), false),
        };
        Scope {
            graph: self.graph.clone(),
            name: new_name,
            children_names: Rc::new(RefCell::new(HashSet::new())),
            op_name: self.op_name.clone(),
            op_names: if copy_names {
                self.op_names.clone()
            } else {
                Rc::new(RefCell::new(HashMap::new()))
            },
        }
    }

    /// Return a new scope. All ops created within the returned scope will have
    /// names of the form `scope_name/name[_suffix]`
    pub fn with_op_name(&self, name: &str) -> Scope {
        Scope {
            graph: self.graph.clone(),
            name: self.name.clone(),
            children_names: self.children_names.clone(),
            op_name: name.to_string(),
            op_names: self.op_names.clone(),
        }
    }

    /// Return a unique name, using default_name if an op name has not been
    /// specified.
    pub fn get_unique_name_for_op(&self, default_name: &str) -> String {
        let name = if self.op_name == "" {
            default_name
        } else {
            &self.op_name
        };
        let map: &RefCell<_> = self.op_names.borrow();
        let mut map = map.borrow_mut();
        let mut name_string = name.to_string();
        loop {
            match map.entry(name_string.clone()) {
                Entry::Vacant(e) => {
                    e.insert(0);
                    return join("/", &self.name, &name_string);
                }
                Entry::Occupied(mut e) => {
                    *e.get_mut() += 1;
                    name_string = format!("{}_{}", name, *e.get());
                }
            }
        }
    }

    /// Returns the graph being built by the scope.
    pub fn graph(&self) -> impl Deref<Target = Graph> + '_ {
        let r: &RefCell<Graph> = self.graph.borrow();
        r.borrow()
    }

    /// Returns the graph being built by the scope.
    pub fn graph_mut(&mut self) -> impl DerefMut<Target = Graph> + '_ {
        let r: &RefCell<Graph> = self.graph.borrow();
        r.borrow_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataType;

    #[test]
    fn smoke() {
        let mut scope = Scope::new_root_scope();
        let mut graph = scope.graph_mut();
        let mut c = graph.new_operation("Const", "Const").unwrap();
        c.set_attr_tensor("value", 3.0f32.into()).unwrap();
        c.set_attr_type("dtype", DataType::Float).unwrap();
        c.finish().unwrap();
    }

    #[test]
    fn uniquification() {
        let scope = Scope::new_root_scope();
        assert_eq!(&scope.new_sub_scope("foo").name, "foo");
        assert_eq!(&scope.new_sub_scope("foo").name, "foo_1");
        let foo_1 = scope.new_sub_scope("foo");
        assert_eq!(&foo_1.name, "foo_2");
        assert_eq!(&foo_1.new_sub_scope("bar").name, "foo_2/bar");
        assert_eq!(&foo_1.new_sub_scope("bar").name, "foo_2/bar_1");
        assert_eq!(&foo_1.new_sub_scope("bar").name, "foo_2/bar_2");
    }

    #[test]
    fn get_unique_name_for_op() {
        let scope = Scope::new_root_scope();
        assert_eq!(scope.get_unique_name_for_op("Add"), "Add");
        assert_eq!(scope.get_unique_name_for_op("Add"), "Add_1");
        let foo = scope.new_sub_scope("foo");
        assert_eq!(foo.get_unique_name_for_op("Add"), "foo/Add");
        assert_eq!(foo.get_unique_name_for_op("Add"), "foo/Add_1");
        let bar = foo.with_op_name("bar");
        assert_eq!(bar.get_unique_name_for_op("Add"), "foo/bar");
        assert_eq!(bar.get_unique_name_for_op("Add"), "foo/bar_1");
    }
}
