import { useState, useEffect } from 'react';

export const useTheme = () => {
  const [theme, setTheme] = useState<string>(() => {
    // Check localStorage for saved theme
    const savedTheme = localStorage.getItem('bridge-theme');
    if (savedTheme) {
      return savedTheme;
    }
    
    // Check system preference
    if (window.matchMedia('(prefers-color-scheme: dark)').matches) {
      return 'dark';
    }
    
    return 'light'; // default
  });

  useEffect(() => {
    // Update the HTML class for CSS variables
    document.documentElement.className = theme;
    // Save to localStorage
    localStorage.setItem('bridge-theme', theme);
  }, [theme]);

  const toggleTheme = () => {
    setTheme(prev => (prev === 'dark' ? 'light' : 'dark'));
  };

  return { theme, toggleTheme };
};
