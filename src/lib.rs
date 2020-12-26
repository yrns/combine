use futures_signals::signal::Signal;
use pin_project::pin_project;
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context, Poll};

pub enum Input<T> {
    Changed(T),
    NotChanged(T),
}

impl<T> std::ops::Deref for Input<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Changed(v) | Self::NotChanged(v) => v,
        }
    }
}

trait PollAll<T> {
    fn poll_all(self: Pin<&mut Self>, cx: &mut Context) -> Option<T>;
}

pub trait Combine<F, T> {
    fn combine(self, f: F) -> T;
}

pub fn combine<S, T: Combine<F, S>, F>(inputs: T, f: F) -> S {
    inputs.combine(f)
}

macro_rules! impl_combine {
    ($Inputs:ident, $Combine:ident, ($(($id:ident, $dc:ident, $i:tt, $None:expr)),+)) => {

        impl<$($id: Signal),+> PollAll<($(Option<$id::Item>),+)> for $Inputs<$($id),+>
        {
            fn poll_all(
                self: Pin<&mut Self>,
                cx: &mut Context,
            ) -> Option<($(Option<$id::Item>),+)> {
                let mut done = true;
                let mut this = self.project();
                let p = ($(
                    {
                        match this.$i.as_mut().as_pin_mut().map(|s| s.poll_change(cx)) {
                            None => None,
                            Some(Poll::Pending) => {
                                done = false;
                                None
                            }
                            Some(Poll::Ready(None)) => {
                                this.$i.set(None);
                                None
                            }
                            Some(Poll::Ready(a)) => {
                                done = false;
                                a
                            }
                        }
                    }
                ),+);

                if done {
                    None
                } else {
                    Some(p)
                }
            }
        }

        // This exists solely for projection since pin-project doesn't handle
        // tuple struct members.
        #[pin_project]
        #[derive(Debug)]
        struct $Inputs<$($id),+>($(#[pin] Option<$id>),+);

        #[pin_project]
        #[derive(Debug)]
        pub struct $Combine<$($id: Signal),+, Item, CombineFn>
        where
            CombineFn: Fn($(Input<&$id::Item>),+) -> Item,
        {
            #[pin]
            inputs: $Inputs<$($id),+>,
            last: ($(Option<$id::Item>),+),
            f: CombineFn,
        }

        impl<$($id: Signal),+, Item, CombineFn> Combine<CombineFn, $Combine<$($id),+, Item, CombineFn>> for ($($id),+)
        where
            CombineFn: Fn($(Input<&$id::Item>),+) -> Item,
        {
            fn combine(self, f: CombineFn) -> $Combine<$($id),+, Item, CombineFn> {
                $Combine {
                    inputs: $Inputs($(Some(self.$i)),+),
                    last: ($($None),+),
                    f,
                }
            }
        }

        impl<$($id: Signal),+, Item, CombineFn> Signal for $Combine<$($id),+, Item, CombineFn>
        where
            CombineFn: Fn($(Input<&$id::Item>),+) -> Item,
        {
            type Item = Item;

            fn poll_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
                let mut this = self.project();
                let mut last = &mut this.last;

                match this.inputs.poll_all(cx) {
                    Some(p) => {
                        let ready = ($(
                            match p.$i {
                                Some(a) => {
                                    last.$i = Some(a);
                                    last.$i.as_ref().map(Input::Changed)
                                }
                                None => last.$i.as_ref().map(Input::NotChanged),
                            }
                        ),+);

                        if let ($(Some($dc)),+) = ready {
                            Poll::Ready(Some((this.f)($($dc),+)))
                        } else {
                            Poll::Pending
                        }
                    }
                    _ => Poll::Ready(None),
                }
            }
        }

    };
}

impl_combine!(Inputs2, Combine2, ((A, a, 0, None), (B, b, 1, None)));
impl_combine!(
    Inputs3,
    Combine3,
    ((A, a, 0, None), (B, b, 1, None), (C, c, 2, None))
);

#[cfg(test)]
mod tests {
    use super::{Combine, Input};
    use futures_lite::{future::block_on, StreamExt};
    use futures_signals::signal::{Mutable, SignalExt};

    #[test]
    fn combine2() {
        let m = Mutable::new(1usize);
        let s1 = m.signal().map(|v| v * 40);
        let s2 = m.signal().map(|v| v + 1);
        let sum = (s1, s2).combine(|a, b| *a + *b).to_stream();

        block_on(async {
            assert_eq!(sum.take(1).collect::<Vec<_>>().await, vec![42]);
        });
    }

    #[test]
    fn changes() {
        let m1 = Mutable::new(0usize);
        let m2 = Mutable::new(0usize);
        let mut changes = (m1.signal(), m2.signal())
            .combine(|a, b| match (a, b) {
                (Input::Changed(_), Input::Changed(_)) => 3,
                (Input::NotChanged(_), Input::Changed(_)) => 2,
                (Input::Changed(_), Input::NotChanged(_)) => 1,
                _ => panic!(0),
            })
            .to_stream();

        block_on(async {
            assert_eq!(changes.next().await, Some(3));
            m2.set(1);
            assert_eq!(changes.next().await, Some(2));
            m1.set(1);
            assert_eq!(changes.next().await, Some(1));
        });
    }

    #[test]
    fn combine3() {
        let m = Mutable::new(1usize);
        let s1 = m.signal().map(|v| v * 15);
        let s2 = m.signal().map(|v| v * 25);
        let s3 = m.signal().map(|v| v + 1);
        let sum = (s1, s2, s3).combine(|a, b, c| *a + *b + *c).to_stream();

        block_on(async {
            assert_eq!(sum.take(1).collect::<Vec<_>>().await, vec![42]);
        });
    }
}
