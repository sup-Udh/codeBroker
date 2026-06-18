from app.utils.email_utils import send_email_sync
from app.repositories.audit_repository import AuditRepository

class EmailService:
    def __init__(self, audit_repo: AuditRepository):
        self.audit_repo = audit_repo

    def send_welcome_email(self, email: str, user_id: int):
        send_email_sync(email, "Welcome to PyShop!", "Thank you for joining.")
        self.audit_repo.log_action("SEND_EMAIL", "Welcome", user_id)
        
    def send_order_confirmation(self, email: str, order_id: int, user_id: int):
        send_email_sync(email, f"Order {order_id} Confirmed", "Your order is processing.")
        self.audit_repo.log_action("SEND_EMAIL", f"Order_{order_id}", user_id)
