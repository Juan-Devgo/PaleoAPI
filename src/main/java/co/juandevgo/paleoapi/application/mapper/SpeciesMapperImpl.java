package co.juandevgo.paleoapi.application.mapper;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.domain.Specie;
import co.juandevgo.paleoapi.infrastructure.entity.SpecieEntity;
import org.springframework.stereotype.Component;

@Component
public class SpeciesMapperImpl implements SpeciesMapper {

    @Override
    public SpecieDTO toDTO(SpecieEntity entity) {
        SpecieDTO dto = new SpecieDTO();
        dto.setName(entity.getName());
        dto.setGroup(entity.getGroup());
        dto.setPhonetic(entity.getPhonetic());
        dto.setDiet(entity.getDiet());
        dto.setSize(entity.getSize());
        dto.setTimePeriod(entity.getTimePeriod());
        dto.setLocations(entity.getLocations());
        dto.setClassification(entity.getClassification());
        dto.setImages(entity.getImages());

        return dto;
    }

    @Override
    public SpecieDTO toDTO(Specie specie) {
        SpecieDTO dto = new SpecieDTO();
        dto.setName(specie.getName());
        dto.setGroup(specie.getGroup());
        dto.setPhonetic(specie.getPhonetic());
        dto.setDiet(specie.getDiet());
        dto.setSize(specie.getSize());
        dto.setTimePeriod(specie.getTimePeriod());
        dto.setLocations(specie.getLocations());
        dto.setClassification(specie.getClassification());
        dto.setImages(specie.getImages());

        return dto;
    }

    @Override
    public SpecieEntity toEntity(SpecieDTO dto) {
        SpecieEntity entity = new SpecieEntity();
        entity.setName(dto.getName());
        entity.setGroup(dto.getGroup());
        entity.setPhonetic(dto.getPhonetic());
        entity.setDiet(dto.getDiet());
        entity.setSize(dto.getSize());
        entity.setTimePeriod(dto.getTimePeriod());
        entity.setLocations(dto.getLocations());
        entity.setClassification(dto.getClassification());
        entity.setImages(dto.getImages());

        return entity;
    }

    @Override
    public SpecieEntity toEntity(Specie specie) {
        SpecieEntity entity = new SpecieEntity();
        entity.setName(specie.getName());
        entity.setGroup(specie.getGroup());
        entity.setPhonetic(specie.getPhonetic());
        entity.setDiet(specie.getDiet());
        entity.setSize(specie.getSize());
        entity.setTimePeriod(specie.getTimePeriod());
        entity.setLocations(specie.getLocations());
        entity.setClassification(specie.getClassification());
        entity.setImages(specie.getImages());

        return entity;
    }

    @Override
    public Specie toDomainEntity(SpecieEntity entity) {
        return Specie.builder()
                .id(entity.getId())
                .name(entity.getName())
                .group(entity.getGroup())
                .phonetic(entity.getPhonetic())
                .diet(entity.getDiet())
                .size(entity.getSize())
                .timePeriod(entity.getTimePeriod())
                .locations(entity.getLocations())
                .classification(entity.getClassification())
                .images(entity.getImages())
                .build();
    }

    @Override
    public Specie toDomainEntity(SpecieDTO dto) {
        return Specie.builder()
                .name(dto.getName())
                .group(dto.getGroup())
                .phonetic(dto.getPhonetic())
                .diet(dto.getDiet())
                .size(dto.getSize())
                .timePeriod(dto.getTimePeriod())
                .locations(dto.getLocations())
                .classification(dto.getClassification())
                .images(dto.getImages())
                .build();
    }
}