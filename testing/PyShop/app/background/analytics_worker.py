from app.services.analytics_service import AnalyticsService

def run_nightly_analytics(analytics: AnalyticsService):
    analytics.generate_daily_report()
