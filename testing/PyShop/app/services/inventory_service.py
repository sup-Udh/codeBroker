from sqlalchemy.orm import Session
from app.repositories.inventory_repository import InventoryRepository
from app.repositories.audit_repository import AuditRepository

class InventoryService:
    def __init__(self, db: Session):
        self.inv_repo = InventoryRepository(db)
        self.audit_repo = AuditRepository(db)

    def check_availability(self, product_id: int, required_qty: int) -> bool:
        inv = self.inv_repo.get_by_product_id(product_id)
        return inv is not None and inv.quantity >= required_qty

    def deduct_stock(self, product_id: int, quantity: int, user_id: int):
        inv = self.inv_repo.get_by_product_id(product_id)
        if not inv or inv.quantity < quantity:
            raise ValueError("Insufficient stock")
        
        self.inv_repo.update_quantity(product_id, inv.quantity - quantity)
        self.audit_repo.log_action("DEDUCT_STOCK", f"Product_{product_id}", user_id)
