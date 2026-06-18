from sqlalchemy.orm import Session
from app.repositories.user_repository import UserRepository
from app.repositories.audit_repository import AuditRepository
from app.schemas.user_schema import UserCreate
from app.config.security import get_password_hash
from app.services.email_service import EmailService

class UserService:
    def __init__(self, db: Session):
        self.user_repo = UserRepository(db)
        self.audit_repo = AuditRepository(db)
        self.email_service = EmailService(self.audit_repo)

    def register_user(self, user_data: UserCreate):
        existing = self.user_repo.get_user_by_email(user_data.email)
        if existing:
            raise ValueError("Email already registered")
        
        hashed = get_password_hash(user_data.password)
        user = self.user_repo.create_user(user_data.email, hashed)
        
        self.audit_repo.log_action("REGISTER", "User", user.id)
        self.email_service.send_welcome_email(user.email, user.id)
        return user
