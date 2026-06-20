<script setup lang="ts">
defineProps<{
  title: string;
  value: string | number;
  highlight?: boolean;
}>();
</script>

<template>
  <div class="card" :class="{ highlight }">
    <h3>{{ title }}</h3>
    <div class="value">{{ value }}</div>
  </div>
</template>

<style scoped>
.card {
  padding: 1.5rem;
  background: white;
  border-radius: 8px;
  box-shadow: 0 2px 4px rgba(0,0,0,0.1);
  text-align: center;
}
.highlight {
  border-left: 4px solid #007bff;
}
h3 {
  margin: 0 0 0.5rem 0;
  color: #666;
}
.value {
  font-size: 2rem;
  font-weight: bold;
  color: #333;
}
</style>
