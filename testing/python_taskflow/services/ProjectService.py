from repositories.ProjectRepository import ProjectRepository
from services.NotificationService import NotificationService
from utils.logger import get_logger

logger = get_logger(__name__)

class ProjectService:
    def __init__(self):
        self.repo = ProjectRepository()
        self.notification_service = NotificationService()
    
    def get_project(self, project_id: int):
        logger.info(f"ProjectService: Getting project {project_id}")
        self.notification_service.send_notification(1, "Project accessed")
        return self.repo.get_by_id(project_id)
