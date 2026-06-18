from fastapi import FastAPI
from app.config.database import engine, Base
from app.routes import auth, users, products, orders, inventory, admin
from app.middleware.logging_middleware import LoggingMiddleware

# Create tables
Base.metadata.create_all(bind=engine)

app = FastAPI(title="PyShop API", description="Stress testing CodeBroker", version="1.0.0")

# Middleware
app.add_middleware(LoggingMiddleware)

# Routers
app.include_router(auth.router)
app.include_router(users.router)
app.include_router(products.router)
app.include_router(orders.router)
app.include_router(inventory.router)
app.include_router(admin.router)

@app.get("/")
def root():
    return {"message": "Welcome to PyShop API"}
