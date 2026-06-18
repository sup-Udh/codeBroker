from sqlalchemy.orm import Session
from app.repositories.order_repository import OrderRepository
from app.repositories.audit_repository import AuditRepository
from app.services.inventory_service import InventoryService
from app.services.email_service import EmailService
from app.schemas.order_schema import OrderCreate

class OrderService:
    def __init__(self, db: Session):
        self.order_repo = OrderRepository(db)
        self.audit_repo = AuditRepository(db)
        self.inventory_service = InventoryService(db)
        self.email_service = EmailService(self.audit_repo)

    def place_order(self, user_id: int, user_email: str, order_data: OrderCreate):
        # Check all inventory first
        for pid in order_data.product_ids:
            if not self.inventory_service.check_availability(pid, 1):
                raise ValueError(f"Product {pid} out of stock")
                
        # Deduct stock
        for pid in order_data.product_ids:
            self.inventory_service.deduct_stock(pid, 1, user_id)
            
        # Create Order
        order = self.order_repo.create_order(user_id, order_data.total_amount)
        
        self.audit_repo.log_action("PLACE_ORDER", f"Order_{order.id}", user_id)
        self.email_service.send_order_confirmation(user_email, order.id, user_id)
        
        return order
