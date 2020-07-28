use easy_parallel::Parallel;
use futures::stream::StreamExt;
use futures_signals::signal::{Mutable, Signal, SignalExt};
use pin_project::pin_project;
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context, Poll};

#[pin_project(project = CombineProj)]
struct Combine<A, B, F>
where
    A: Signal,
    B: Signal,
{
    #[pin]
    a: A,
    #[pin]
    b: B,
    last_a: Option<A::Item>,
    last_b: Option<B::Item>,
    f: F,
}

fn combine<A, B, C, F>(a: A, b: B, f: F) -> Combine<A, B, F>
where
    A: Signal,
    B: Signal,
    F: Fn(A::Item, B::Item) -> C,
{
    Combine {
        a,
        b,
        last_a: None,
        last_b: None,
        f,
    }
}

impl<A, B, C, F> Signal for Combine<A, B, F>
where
    A: Signal,
    B: Signal,
    F: Fn(A::Item, B::Item) -> C,
    A::Item: Debug,
    B::Item: Debug,
    C: Debug,
{
    type Item = C;

    fn poll_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let CombineProj {
            a,
            b,
            last_a,
            last_b,
            f,
        } = self.project();

        match dbg!((
            a.poll_change(cx),
            b.poll_change(cx),
            last_a.take(),
            last_b.take()
        )) {
            (Poll::Ready(None), _, _, _) | (_, Poll::Ready(None), _, _) => Poll::Ready(None),
            (Poll::Ready(Some(a)), Poll::Pending, _, Some(b))
            | (Poll::Pending, Poll::Ready(Some(b)), Some(a), _)
            | (Poll::Ready(Some(a)), Poll::Ready(Some(b)), _, _) => Poll::Ready(Some(f(a, b))),
            (Poll::Ready(Some(a)), Poll::Pending, _, None) => {
                *last_a = Some(a);
                Poll::Pending
            }
            (Poll::Pending, Poll::Ready(Some(b)), None, _) => {
                *last_b = Some(b);
                Poll::Pending
            }
            (Poll::Pending, Poll::Pending, a, b) => {
                *last_a = a;
                *last_b = b;
                Poll::Pending
            }
        }
    }
}

fn main() {
    let m = Mutable::new(0usize);
    let s1 = m.signal().map(|v| v * 2);
    let s2 = m.signal().map(|v| v + 1);
    let mut sum = combine(s1, s2, |a, b| a + b).to_stream();

    Parallel::new()
        .add(|| {
            futures::executor::block_on(async {
                assert_eq!(sum.next().await, Some(1usize));
                assert_eq!(sum.next().await, Some(7usize));
                //assert_eq!(sum.next().await, None);
            });
        })
        .add(|| {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            dbg!("set to 1!");
            m.set(2);
        })
        .run();
}
