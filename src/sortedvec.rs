use std::ops::{Deref, DerefMut};

pub struct SortedVec<T: Ord> {
    data: Vec<T>,
}

impl<T: Ord> SortedVec<T> {
    pub fn new() -> SortedVec<T> {
        SortedVec {
            data: vec!(),
        }
    }

    pub fn insert(&mut self, val: T) {
        let mut left = 0;
        let mut right = self.data.len();

        while left != right {
            let (middle, rem) = ((left + right) / 2, (left + right) % 2);
            if val <= self.data[middle] {
                right = middle;
            } else {
                left = middle + rem;
            }
        }

        if self.data.len() == 0 || left == self.data.len() || self.data[left] != val {
            self.data.insert(left, val);
        }
    }

    pub fn contains(&self, val: &T) -> bool {
        let mut left = 0;
        let mut right = self.data.len();

        while left != right {
            let (middle, rem) = ((left + right) / 2, (left + right) % 2);
            if val <= &self.data[middle] {
                right = middle;
            } else {
                left = middle + rem;
            }
        }

        left < self.data.len() && &self.data[left] == val
    }

    pub fn remove(&mut self, val: &T) -> bool {
        let mut left = 0;
        let mut right = self.data.len();

        while left != right {
            let (middle, rem) = ((left + right) / 2, (left + right) % 2);
            if val <= &self.data[middle] {
                right = middle;
            } else {
                left = middle + rem;
            }
        }

        if left < self.data.len() && &self.data[left] == val {
            self.data.remove(left);
            true
        } else {
            false
        }
    }
}

impl<T: Ord> Deref for SortedVec<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.data
    }
}

impl<T: Ord> DerefMut for SortedVec<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.data
    }
}
