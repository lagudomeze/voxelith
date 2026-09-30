use std::{collections::VecDeque, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key(pub(crate) usize);

#[derive(Clone, PartialEq)]
pub struct SlotVec<T> {
    items: Vec<Option<T>>,
    free: VecDeque<usize>,
}

impl<T> Default for SlotVec<T> {
    fn default() -> Self {
        Self {
            items: Vec::default(),
            free: VecDeque::default(),
        }
    }
}

impl<T> SlotVec<T> {
    #[inline]
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            free: VecDeque::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
            free: VecDeque::with_capacity(capacity),
        }
    }

    #[inline]
    pub fn insert(&mut self, value: T) -> Key {
        if let Some(index) = self.free.pop_back() {
            self.items[index] = Some(value);
            Key(index)
        } else {
            let index = self.items.len();
            self.items.push(Some(value));
            Key(index)
        }
    }

    #[inline]
    pub fn remove(&mut self, key: Key) -> Option<T> {
        if let Some(slot) = self.items.get_mut(key.0) {
            slot.take().inspect(|_| {
                self.free.push_back(key.0);
            })
        } else {
            None
        }
    }

    #[inline]
    pub fn get_ptr(&self, key: &Key) -> Option<*const T> {
        self.items.get(key.0)?.as_ref().map(|item| item as *const T)
    }

    #[inline]
    pub fn get(&self, key: &Key) -> Option<&T> {
        self.items.get(key.0)?.as_ref()
    }

    #[inline]
    pub fn get_mut(&mut self, key: &Key) -> Option<&mut T> {
        self.items.get_mut(key.0)?.as_mut()
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter().filter_map(|x| x.as_ref())
    }

    #[inline]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items.iter_mut().filter_map(|x| x.as_mut())
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.items.len() - self.free.len()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.items.clear();
        self.free.clear();
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl<T: fmt::Debug> fmt::Debug for SlotVec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}
