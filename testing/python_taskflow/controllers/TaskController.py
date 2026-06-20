from services.TaskService import TaskService
from schemas.TaskSchema import TaskResponse
from services.ProjectService import ProjectService
from services.NotificationService import NotificationService
from utils.logger import get_logger

logger = get_logger(__name__)

class TaskController:
    def __init__(self):
        self.service = TaskService()
        self.project_service = ProjectService()
        self.notification_service = NotificationService()

    def get_task(self, task_id: int) -> dict:
        logger.info(f"TaskController: get task {task_id}")
        task = self.service.get_task(task_id)
        return {"id": task_id, "title": "test task"}
