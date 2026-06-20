from repositories.TaskRepository import TaskRepository
from services.ProjectService import ProjectService
from services.NotificationService import NotificationService
from utils.logger import get_logger

logger = get_logger(__name__)

class TaskService:
    def __init__(self):
        self.repo = TaskRepository()
        self.project_service = ProjectService()
        self.notification_service = NotificationService()
    
    def get_task(self, task_id: int):
        logger.info(f"TaskService: Getting task {task_id}")
        self.notification_service.send_notification(1, "Task accessed")
        return self.repo.get_by_id(task_id)
