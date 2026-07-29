//! Barramento tipado e limitado de eventos da aplicação.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use ide_domain::DocumentId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdeEvent {
    WorkspaceOpened {
        root: PathBuf,
    },
    ProjectImported {
        root: PathBuf,
        build_system: String,
    },
    DocumentOpened {
        document_id: DocumentId,
        path: PathBuf,
    },
    DocumentChanged {
        document_id: DocumentId,
        version: u64,
    },
    DocumentClosed(DocumentId),
}

#[derive(Clone)]
pub struct EventBus {
    queue: Arc<Mutex<VecDeque<IdeEvent>>>,
    capacity: usize,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::bounded(256)
    }
}

impl EventBus {
    pub fn bounded(capacity: usize) -> Self {
        assert!(capacity > 0, "event capacity must be positive");
        Self {
            queue: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    pub fn publish(&self, event: IdeEvent) -> Result<(), PublishError> {
        let mut queue = self.queue.lock().map_err(|_| PublishError::Unavailable)?;
        if queue.len() == self.capacity {
            return Err(PublishError::Full);
        }
        queue.push_back(event);
        Ok(())
    }

    pub fn drain(&self) -> Result<Vec<IdeEvent>, PublishError> {
        let mut queue = self.queue.lock().map_err(|_| PublishError::Unavailable)?;
        Ok(queue.drain(..).collect())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishError {
    Full,
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_bus_applies_backpressure() {
        let bus = EventBus::bounded(1);
        assert_eq!(
            bus.publish(IdeEvent::WorkspaceOpened {
                root: PathBuf::from("/workspace")
            }),
            Ok(())
        );
        assert_eq!(
            bus.publish(IdeEvent::DocumentClosed(DocumentId(1))),
            Err(PublishError::Full)
        );
    }

    #[test]
    fn typed_events_preserve_order_and_payload() {
        let bus = EventBus::bounded(3);
        let opened = IdeEvent::DocumentOpened {
            document_id: DocumentId(7),
            path: PathBuf::from("/workspace/Main.java"),
        };
        let changed = IdeEvent::DocumentChanged {
            document_id: DocumentId(7),
            version: 2,
        };
        assert_eq!(bus.publish(opened.clone()), Ok(()));
        assert_eq!(bus.publish(changed.clone()), Ok(()));
        assert_eq!(bus.drain(), Ok(vec![opened, changed]));
    }
}
