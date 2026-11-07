//! Assigns a component prop from either a plain value or an existing [`Reactive`] handle.
//!
//! [`ByValue`] and [`ByHandle`] are inferred. Callers write `prop_a(value)` and do not name them.

use crate::Reactive;

/// Writes `Self` into a prop slot of type `T`.
///
/// `Marker` is [`ByHandle`] when `Self` is [`Reactive<T>`](crate::Reactive), and [`ByValue`] when
/// `Self` converts into `T`. Inference picks one; the two markers keep the implementations from
/// overlapping.
pub trait PropInput<T: 'static, Marker> {
    fn apply(self, slot: &mut Reactive<T>);
}

/// Marker for [`PropInput`]: replace the slot with this handle.
pub struct ByHandle;

/// Marker for [`PropInput`]: [`Reactive::set`](crate::Reactive::set) the converted value.
pub struct ByValue;

impl<T: 'static> PropInput<T, ByHandle> for Reactive<T> {
    fn apply(self, slot: &mut Reactive<T>) {
        *slot = self;
    }
}

impl<T, Value> PropInput<T, ByValue> for Value
where
    T: 'static,
    Value: Into<T>,
{
    fn apply(self, slot: &mut Reactive<T>) {
        slot.set(self.into());
        slot.hold();
    }
}

#[cfg(test)]
mod tests {
    use super::PropInput;
    use crate::Application;

    fn write<T, Value, Marker>(slot: &mut crate::Reactive<T>, value: Value)
    where
        T: 'static,
        Value: PropInput<T, Marker>,
    {
        value.apply(slot);
    }

    #[test]
    fn values_write_through_and_handles_alias() {
        let context = Application::new().context().clone();
        let mut slot = context.reactive(String::from("old"));
        let original = slot.clone();
        write(&mut slot, "next");
        assert_eq!(slot.get(), "next");
        assert_eq!(original.get(), "next");

        let handle = context.reactive(String::from("alias"));
        write(&mut slot, handle.clone());
        assert_eq!(slot, handle);
        handle.set(String::from("updated"));
        assert_eq!(slot.get(), "updated");
        assert_eq!(original.get(), "next");
    }
}
