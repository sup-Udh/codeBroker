<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
</script>

<template>
  <header>
    <nav>
      <RouterLink to="/">Dashboard</RouterLink>
      <RouterLink to="/inventory">Inventory</RouterLink>
      <RouterLink to="/suppliers">Suppliers</RouterLink>
    </nav>
  </header>

  <main>
    <RouterView />
  </main>
</template>

<style scoped>
header {
  line-height: 1.5;
  max-height: 100vh;
  padding: 1rem;
  background-color: #f8f9fa;
  border-bottom: 1px solid #dee2e6;
}

nav {
  display: flex;
  gap: 1rem;
  font-size: 1.2rem;
}

nav a {
  text-decoration: none;
  color: #007bff;
}

nav a.router-link-exact-active {
  color: #0056b3;
  font-weight: bold;
}

main {
  padding: 2rem;
}
</style>
