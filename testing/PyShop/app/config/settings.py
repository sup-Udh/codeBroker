from pydantic_settings import BaseSettings

class Settings(BaseSettings):
    DATABASE_URL: str = "sqlite:///./pyshop.db"
    JWT_SECRET: str = "super_secret_jwt_key_for_testing"
    EMAIL_HOST: str = "smtp.example.com"
    EMAIL_PORT: int = 587
    ENVIRONMENT: str = "development"

settings = Settings()
