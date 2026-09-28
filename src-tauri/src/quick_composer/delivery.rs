use super::QuickLaunch;
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Clone, Serialize)]
pub struct Delivery {
    pub id: String,
    pub request: QuickLaunch,
}

struct Pending {
    delivery: Delivery,
    owner: String,
}

#[derive(Default)]
pub struct LaunchQueue(VecDeque<Pending>);

impl LaunchQueue {
    pub fn has_capacity(&self) -> bool {
        self.0.len() < 64
    }

    pub fn push(&mut self, request: QuickLaunch, owner: String) {
        self.0.push_back(Pending {
            delivery: Delivery {
                id: uuid::Uuid::new_v4().to_string(),
                request,
            },
            owner,
        });
    }

    // Claims are repeatable until acknowledged. Only the owning window can
    // accept a delivery; a destroyed owner can be replaced by a live window.
    pub fn claim(&mut self, window: &str, exists: impl Fn(&str) -> bool) -> Option<Delivery> {
        let next = self
            .0
            .iter_mut()
            .find(|entry| entry.owner == window || !exists(&entry.owner))?;
        next.owner = window.to_owned();
        Some(next.delivery.clone())
    }

    pub fn acknowledge(&mut self, window: &str, id: &str) {
        self.0
            .retain(|entry| !(entry.owner == window && entry.delivery.id == id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(prompt: &str) -> QuickLaunch {
        serde_json::from_value(serde_json::json!({
            "prompt": prompt, "cwd": "/repo", "harness": "codex", "reveal": false
        }))
        .unwrap()
    }
    #[test]
    fn consecutive_submissions_wait_for_successful_acknowledgement() {
        let mut queue = LaunchQueue::default();
        queue.push(request("first"), "booting".into());
        queue.push(request("second"), "booting".into());
        let first = queue.claim("booting", |_| true).unwrap();
        assert_eq!(first.request.prompt, "first");
        assert_eq!(queue.claim("booting", |_| true).unwrap().id, first.id);
        queue.acknowledge("other", &first.id);
        assert_eq!(queue.claim("booting", |_| true).unwrap().id, first.id);
        queue.acknowledge("booting", &first.id);
        let second = queue.claim("booting", |_| true).unwrap();
        assert_eq!(second.request.prompt, "second");
        assert_ne!(first.id, second.id);
        queue.acknowledge("booting", &first.id); // lost ACK response / retry
        assert_eq!(queue.claim("booting", |_| true).unwrap().id, second.id);
        queue.acknowledge("booting", &second.id);
        assert!(queue.claim("booting", |_| true).is_none());
    }
    #[test]
    fn retains_other_windows_requests_and_reassigns_only_destroyed_owners() {
        let mut queue = LaunchQueue::default();
        queue.push(request("one"), "one".into());
        queue.push(request("two"), "two".into());
        assert!(queue.claim("three", |_| true).is_none());
        let two = queue.claim("two", |_| true).unwrap();
        queue.acknowledge("two", &two.id);
        let recovered = queue.claim("three", |owner| owner != "one").unwrap();
        assert_eq!(recovered.request.prompt, "one");
        assert!(queue.claim("two", |_| true).is_none());
    }
}
