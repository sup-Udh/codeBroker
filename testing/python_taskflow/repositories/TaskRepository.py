from models.Task import Task
from utils.logger import get_logger

logger = get_logger(__name__)

class TaskRepository:
    def __init__(self):
        self.db = []
    
    def get_by_id(self, task_id: int) -> Task | None:
        logger.info(f"TaskRepository fetching task {task_id}")
        return next((t for t in self.db if t.id == task_id), None)
