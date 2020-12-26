use futures_signals::signal::Signal;
use pin_project::pin_project;
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context, Poll};

trait PollAll<T> {
    fn poll_all(self: Pin<&mut Self>, cx: &mut Context) -> Option<T>;
}

impl<A, B> PollAll<(Option<A::Item>, Option<B::Item>)> for Inputs2<A, B>
where
    A: Signal,
    B: Signal,
{
    fn poll_all(
        self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Option<(Option<A::Item>, Option<B::Item>)> {
        let mut done = 0;
        let mut this = self.project();
        let p = (
            {
                match this.0.as_mut().as_pin_mut().map(|s| s.poll_change(cx)) {
                    None => {
                        done += 1;
                        None
                    }
                    Some(Poll::Pending) => None,
                    Some(Poll::Ready(None)) => {
                        this.0.set(None);
                        done += 1;
                        None
                    }
                    Some(Poll::Ready(a)) => a,
                }
            },
            {
                match this.1.as_mut().as_pin_mut().map(|s| s.poll_change(cx)) {
                    None => {
                        done += 1;
                        None
                    }
                    Some(Poll::Pending) => None,
                    Some(Poll::Ready(None)) => {
                        this.1.set(None);
                        done += 1;
                        None
                    }
                    Some(Poll::Ready(a)) => a,
                }
            },
        );

        if done == 2 {
            None
        } else {
            Some(p)
        }
    }
}

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

// This exists solely for projection since pin-project doesn't handle
// tuple struct members.
#[pin_project]
#[derive(Debug)]
struct Inputs2<A, B>(#[pin] Option<A>, #[pin] Option<B>);

#[pin_project]
#[derive(Debug)]
pub struct Combine2<A, B, C, F>
where
    A: Signal,
    B: Signal,
    F: Fn(Input<&A::Item>, Input<&B::Item>) -> C,
{
    #[pin]
    inputs: Inputs2<A, B>,
    last: (Option<A::Item>, Option<B::Item>),
    f: F,
}

impl<A, B, C, F> Combine<F, Combine2<A, B, C, F>> for (A, B)
where
    A: Signal,
    B: Signal,
    F: Fn(Input<&A::Item>, Input<&B::Item>) -> C,
{
    fn combine(self, f: F) -> Combine2<A, B, C, F> {
        Combine2 {
            inputs: Inputs2(Some(self.0), Some(self.1)),
            last: (None, None),
            f,
        }
    }
}

pub trait Combine<F, T> {
    fn combine(self, f: F) -> T;
}

pub fn combine<S, T: Combine<F, S>, F>(inputs: T, f: F) -> S {
    inputs.combine(f)
}

impl<A, B, C, F> Signal for Combine2<A, B, C, F>
where
    A: Signal,
    B: Signal,
    F: Fn(Input<&A::Item>, Input<&B::Item>) -> C,
    A::Item: Debug,
    B::Item: Debug,
    C: Debug,
{
    type Item = C;

    fn poll_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        let mut last = &mut this.last;

        match this.inputs.poll_all(cx) {
            Some(p) => {
                let ready = (
                    match p.0 {
                        Some(a) => {
                            last.0 = Some(a);
                            last.0.as_ref().map(Input::Changed)
                        }
                        None => last.0.as_ref().map(Input::NotChanged),
                    },
                    match p.1 {
                        Some(a) => {
                            last.1 = Some(a);
                            last.1.as_ref().map(Input::Changed)
                        }
                        None => last.1.as_ref().map(Input::NotChanged),
                    },
                );

                if let (Some(a), Some(b)) = ready {
                    Poll::Ready(Some((this.f)(a, b)))
                } else {
                    Poll::Pending
                }
            }
            _ => Poll::Ready(None),
        }
    }
}

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
}
