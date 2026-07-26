#![doc = "Barramento tipado e limitado de eventos da aplicação."]

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use ide_domain::{DocumentId, ProjectId, WorkspaceId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdeEvent {
    WorkspaceOpened(WorkspaceId),
    WorkspaceClosed(WorkspaceId),
    ProjectImported(ProjectId),
    DocumentOpened(DocumentId),
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
            bus.publish(IdeEvent::WorkspaceOpened(WorkspaceId(1))),
            Ok(())
        );
        assert_eq!(
            bus.publish(IdeEvent::WorkspaceClosed(WorkspaceId(1))),
            Err(PublishError::Full)
        );
    }
}
