import { useEffect } from "react";
import { Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { AppProvider, useAppState } from "./AppState";
import { StartPage } from "../pages/StartPage";
import { WorkPage } from "../pages/WorkPage";
import { SettingsPage } from "../pages/SettingsPage";

export default function App() {
  return (
    <AppProvider>
      <InboxLaunchBridge />
      <RouteScrollReset />
      <Routes>
        <Route path="/" element={<StartPage />} />
        <Route path="/work" element={<WorkPage />} />
        <Route path="/settings" element={<SettingsPage />} />
      </Routes>
    </AppProvider>
  );
}

function InboxLaunchBridge() {
  const navigate = useNavigate();
  const location = useLocation();
  const { lastInboxReceptionAt } = useAppState();

  useEffect(() => {
    if (!lastInboxReceptionAt) return;
    const target = "/?mode=inbox";
    if (`${location.pathname}${location.search}` !== target) {
      navigate(target);
    }
  }, [lastInboxReceptionAt, location.pathname, location.search, navigate]);

  return null;
}

function RouteScrollReset() {
  const location = useLocation();

  useEffect(() => {
    window.scrollTo({ top: 0, left: 0, behavior: "auto" });
    document.documentElement.scrollTop = 0;
    document.body.scrollTop = 0;
  }, [location.pathname, location.search]);

  return null;
}
