# Ray Entity Recognition Example
# This is sample code showing the native Ray implementation

import ray
from typing import List, Dict
import openai

@ray.remote(num_cpus=2, num_gpus=0)
class DocumentProcessor:
    def __init__(self, api_key: str):
        self.client = openai.OpenAI(api_key=api_key)
    
    def extract_entities(self, document: str) -> Dict[str, List[str]]:
        """Extract entities from document using LLM."""
        response = self.client.chat.completions.create(
            model="gpt-4",
            messages=[
                {"role": "system", "content": "Extract entities (person, organization, location) from the text."},
                {"role": "user", "content": document}
            ]
        )
        
        # Parse response and extract entities
        entities = self._parse_entities(response.choices[0].message.content)
        return entities
    
    def _parse_entities(self, text: str) -> Dict[str, List[str]]:
        # Simplified parsing
        return {
            "person": ["John Doe", "Jane Smith"],
            "organization": ["Acme Corp"],
            "location": ["New York", "San Francisco"]
        }

@ray.remote
class ResultAggregator:
    def __init__(self):
        self.results = []
    
    def add_result(self, result: Dict[str, List[str]]):
        self.results.append(result)
    
    def get_all_results(self) -> List[Dict[str, List[str]]]:
        return self.results

def main():
    ray.init()
    
    # Create document processor actors
    processors = [
        DocumentProcessor.remote(api_key="sk-...")
        for _ in range(4)  # 4 parallel processors
    ]
    
    # Create aggregator
    aggregator = ResultAggregator.remote()
    
    # Documents to process
    documents = [
        "John Doe works at Acme Corp in New York.",
        "Jane Smith visited San Francisco last week.",
        # ... more documents
    ]
    
    # Process documents in parallel
    futures = []
    for i, doc in enumerate(documents):
        processor = processors[i % len(processors)]
        future = processor.extract_entities.remote(doc)
        futures.append(future)
    
    # Wait for all results
    results = ray.get(futures)
    
    # Aggregate results
    for result in results:
        aggregator.add_result.remote(result)
    
    # Get final aggregated results
    all_results = ray.get(aggregator.get_all_results.remote())
    
    print(f"Processed {len(all_results)} documents")
    ray.shutdown()

if __name__ == "__main__":
    main()
