use super::*;
use ai_terminal_engine::scrollback::Scrollback;
use ai_terminal_protocol::local::{SCROLLBACK_BUSY, SCROLLBACK_EXPIRED};

const TTL: Duration = Duration::from_secs(30);
const MAX_VIEWS: usize = 4;
const MAX_BYTES: usize = 128 * 1024 * 1024;

struct View {
    id: u64,
    touched: Instant,
    history: Scrollback,
}
#[derive(Default)]
pub(super) struct Views(HashMap<u64, View>);
impl Views {
    pub fn expire(&mut self, engine: &Engine) {
        self.0
            .retain(|_, view| view.touched.elapsed() < TTL && view.history.compatible(engine));
    }
    pub fn touch(&mut self, client: u64) {
        if let Some(view) = self.0.get_mut(&client) {
            view.touched = Instant::now();
        }
    }
    pub fn release(&mut self, client: u64) {
        self.0.remove(&client);
    }
    pub fn release_view(&mut self, client: u64, id: u64) {
        if self.0.get(&client).is_some_and(|view| view.id == id) {
            self.release(client);
        }
    }
    pub fn read(&mut self, req: &Request, engine: &Engine) -> Result<Reply> {
        self.expire(engine);
        if req.scrollback_id == 0 {
            self.release(req.client);
            anyhow::ensure!(self.0.len() < MAX_VIEWS, SCROLLBACK_BUSY);
            let history = engine.capture_scrollback()?;
            anyhow::ensure!(
                self.0.values().map(|v| v.history.bytes).sum::<usize>() + history.bytes
                    <= MAX_BYTES,
                SCROLLBACK_BUSY
            );
            self.0.insert(
                req.client,
                View {
                    id: random_id(),
                    touched: Instant::now(),
                    history,
                },
            );
        }
        let view = self.0.get(&req.client).context(SCROLLBACK_EXPIRED)?;
        anyhow::ensure!(
            req.scrollback_id == 0 || req.scrollback_id == view.id,
            SCROLLBACK_EXPIRED
        );
        if req.history_limit > 0 {
            let (history, next) = view
                .history
                .text_page(req.history_offset, req.history_limit)?;
            return Ok(Reply {
                history,
                scrollback_id: view.id,
                history_total: view.history.line_count(),
                history_next: next,
                history_has_more: next < view.history.line_count(),
                history_truncated: view.history.truncated,
                ..Reply::default()
            });
        }
        let (snapshot, offset) = view.history.page(req.history_offset)?;
        Ok(Reply {
            snapshot: Some(snapshot),
            scrollback_id: view.id,
            scrollback_offset: offset,
            scrollback_total: view.history.total(),
            history_truncated: view.history.truncated,
            ..Reply::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn views_are_private_bounded_and_released() {
        let mut engine = Engine::new(3, 10, 1).unwrap();
        engine.feed(b"a\r\nb\r\nc\r\nd\r\n");
        let before = engine.snapshot();
        let mut views = Views::default();
        let req = Request {
            client: 1,
            history_offset: 2,
            ..Request::default()
        };
        let first = views.read(&req, &engine).unwrap();
        let text = views
            .read(
                &Request {
                    scrollback_id: first.scrollback_id,
                    history_offset: 0,
                    history_limit: 200,
                    ..req.clone()
                },
                &engine,
            )
            .unwrap();
        assert_eq!(text.history_total, first.scrollback_total + 3);
        assert_eq!(text.history_next, text.history_total);
        assert!(!text.history_has_more);
        views.release_view(1, first.scrollback_id.wrapping_add(1));
        assert!(views.0.contains_key(&1));
        let mut page = Request {
            scrollback_id: first.scrollback_id,
            ..req.clone()
        };
        assert_eq!(views.read(&page, &engine).unwrap().snapshot, first.snapshot);
        page.client = 2;
        assert!(views.read(&page, &engine).is_err());
        for client in 2..=4 {
            views
                .read(
                    &Request {
                        client,
                        ..req.clone()
                    },
                    &engine,
                )
                .unwrap();
        }
        assert!(
            views
                .read(
                    &Request {
                        client: 5,
                        ..req.clone()
                    },
                    &engine
                )
                .is_err()
        );
        views.release(1);
        assert_eq!(views.0.len(), 3);
        for view in views.0.values_mut() {
            view.touched = Instant::now() - TTL;
        }
        views.expire(&engine);
        assert!(views.0.is_empty());
        assert_eq!(engine.snapshot(), before);
        views.read(&req, &engine).unwrap();
        engine.resize(4, 10).unwrap();
        views.expire(&engine);
        assert!(views.0.is_empty());
    }
}
