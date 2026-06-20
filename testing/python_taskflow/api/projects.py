from fastapi import APIRouter
from controllers.ProjectController import ProjectController

router = APIRouter()
controller = ProjectController()

@router.get("/projects/{project_id}")
def get_project(project_id: int):
    return controller.get_project(project_id)
