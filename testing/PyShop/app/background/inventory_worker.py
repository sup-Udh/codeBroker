from app.services.inventory_service import InventoryService

def restock_low_inventory(inv_service: InventoryService):
    print("Checking for low stock...")
