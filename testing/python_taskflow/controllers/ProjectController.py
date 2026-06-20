from services.ProjectService import ProjectService
from schemas.ProjectSchema import ProjectResponse
from services.NotificationService import NotificationService
from utils.logger import get_logger

logger = get_logger(__name__)

class ProjectController:
    def __init__(self):
        self.service = ProjectService()
        self.notification_service = NotificationService()

    def get_project(self, project_id: int) -> dict:
        logger.info(f"ProjectController: get project {project_id}")
        project = self.service.get_project(project_id)
        return {"id": project_id, "name": "test project"}
